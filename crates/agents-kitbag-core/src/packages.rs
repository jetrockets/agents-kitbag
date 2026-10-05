//! How an npm-distributed MCP server is started.
//!
//! On Windows, Claude Desktop times out an MCP server's initialize after
//! 60 s. The `cmd.exe /C npx -y <pkg>` chain takes 70-85 s cold (registry
//! lookup, tarball download, extract, node spawn), so every restart loses the
//! race. Pre-warming does not fix it: `npx -y` re-checks registry metadata on
//! each run, and tagged versions (`@beta`) always hit the network. So on
//! Windows the package is installed globally up front and the config gets a
//! direct `node <bin path>` command: spawn cost drops to about 300 ms.
//!
//! macOS keeps the upstream-recommended `npx -y`: a cold start fits the
//! timeout there, and no global install is left behind.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::ServerConfig;
use crate::exec::CommandRunner;
use crate::platform::{Env, Os};

pub trait Packages: Send + Sync {
    /// The command that starts `spec` with `extra` arguments.
    fn entry(&self, spec: &str, extra: &[&str]) -> Result<ServerConfig, String>;
}

pub struct SystemPackages<'a> {
    pub env: &'a Env,
    pub runner: &'a dyn CommandRunner,
}

/// `@scope/name` from `@scope/name@beta`, `name` from `name@1.2.3`.
pub fn parse_package_name(spec: &str) -> &str {
    let start = if spec.starts_with('@') {
        match spec.find('/') {
            Some(slash) => slash,
            None => return spec,
        }
    } else {
        0
    };
    match spec[start..].find('@') {
        Some(at) if start + at > 0 => &spec[..start + at],
        _ => spec,
    }
}

/// The script a package runs as: its `bin` string, or of several the one
/// named after the package, then one starting `mcp-`, then the first.
pub fn bin_from_package_json(package: &Value, package_name: &str) -> Result<String, String> {
    match &package["bin"] {
        Value::String(bin) => Ok(bin.clone()),
        Value::Object(bins) if !bins.is_empty() => {
            let short = package_name.rsplit('/').next().unwrap_or(package_name);
            bins.iter()
                .find(|(name, _)| *name == short)
                .or_else(|| bins.iter().find(|(name, _)| name.starts_with("mcp-")))
                .or_else(|| bins.iter().next())
                .and_then(|(_, path)| path.as_str())
                .map(str::to_owned)
                .ok_or_else(|| format!("Package {package_name} has a 'bin' that is not a path"))
        }
        _ => Err(format!(
            "Package {package_name} has no 'bin' entry in package.json"
        )),
    }
}

impl SystemPackages<'_> {
    /// npm is a `.cmd` shim on Windows, which only a shell can start. Every
    /// argument is a literal from an integration, never something typed.
    fn npm(&self, args: &[&str]) -> crate::exec::Output {
        let mut all = vec!["/C", "npm"];
        all.extend_from_slice(args);
        self.runner.run("cmd", &all, None)
    }

    fn node(&self) -> Result<PathBuf, String> {
        self.env
            .on_path("node")
            .or_else(|| {
                Env::first_existing(self.env.path_under("ProgramFiles", &["nodejs", "node.exe"]))
            })
            .ok_or_else(|| "Node.js was not found. Install it from https://nodejs.org/".to_owned())
    }

    fn install_and_resolve(&self, spec: &str) -> Result<ServerConfig, String> {
        let name = parse_package_name(spec);
        let node = self.node()?;
        let installed = self.npm(&[
            "install",
            "-g",
            spec,
            "--no-audit",
            "--no-fund",
            "--no-progress",
        ]);
        if !installed.ok() {
            let tail: Vec<&str> = installed.stderr.trim().lines().rev().take(3).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            return Err(format!(
                "'npm install -g {spec}' failed (exit {}): {}",
                installed.code,
                tail.join(" ")
            ));
        }
        let root = self.npm(&["root", "-g"]);
        if !root.ok() {
            return Err(format!("'npm root -g' exited with code {}", root.code));
        }
        let mut package_dir = PathBuf::from(root.stdout.trim());
        package_dir.extend(name.split('/'));
        let manifest = std::fs::read_to_string(package_dir.join("package.json"))
            .map_err(|e| format!("Installed {name} but its package.json cannot be read: {e}"))?;
        let manifest: Value = serde_json::from_str(&manifest).map_err(|e| e.to_string())?;
        let bin = package_dir.join(Path::new(&bin_from_package_json(&manifest, name)?));
        if !bin.exists() {
            return Err(format!("bin file not found: {}", bin.display()));
        }
        Ok(ServerConfig::new(
            node.to_string_lossy(),
            [bin.to_string_lossy().into_owned()],
        ))
    }
}

impl Packages for SystemPackages<'_> {
    fn entry(&self, spec: &str, extra: &[&str]) -> Result<ServerConfig, String> {
        let mut server = match self.env.os {
            Os::Mac | Os::Linux => ServerConfig::new("npx", ["-y", spec]),
            Os::Windows => self.install_and_resolve(spec)?,
        };
        server.args.extend(extra.iter().map(|a| (*a).to_owned()));
        Ok(server)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, fail, ok};
    use serde_json::json;

    #[test]
    fn package_names_lose_their_version_or_tag() {
        assert_eq!(
            parse_package_name("@roychri/mcp-server-asana@beta"),
            "@roychri/mcp-server-asana"
        );
        assert_eq!(
            parse_package_name("@notionhq/notion-mcp-server"),
            "@notionhq/notion-mcp-server"
        );
        assert_eq!(parse_package_name("mcp-remote"), "mcp-remote");
        assert_eq!(parse_package_name("mcp-remote@1.2.3"), "mcp-remote");
        assert_eq!(parse_package_name("@scope"), "@scope");
    }

    #[test]
    fn the_bin_is_chosen_by_name_then_mcp_prefix_then_first() {
        let name = "@azure-devops/mcp";
        assert_eq!(
            bin_from_package_json(&json!({"bin": "dist/a.js"}), name).unwrap(),
            "dist/a.js"
        );
        assert_eq!(
            bin_from_package_json(&json!({"bin": {"other": "o.js", "mcp": "m.js"}}), name).unwrap(),
            "m.js"
        );
        assert_eq!(
            bin_from_package_json(
                &json!({"bin": {"other": "o.js", "mcp-server-x": "s.js"}}),
                name
            )
            .unwrap(),
            "s.js"
        );
        assert_eq!(
            bin_from_package_json(&json!({"bin": {"zeta": "z.js", "alpha": "a.js"}}), name)
                .unwrap(),
            "z.js"
        );
        assert!(bin_from_package_json(&json!({}), name).is_err());
    }

    #[test]
    fn macos_starts_the_package_through_npx() {
        let env = Env::with(Os::Mac, "/h", &[]);
        let runner = FakeRunner::default();
        let packages = SystemPackages {
            env: &env,
            runner: &runner,
        };
        let server = packages
            .entry("mcp-remote", &["https://mcp.linear.app/mcp"])
            .unwrap();
        assert_eq!(server.command, "npx");
        assert_eq!(
            server.args,
            ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
        );
        assert!(runner.calls().is_empty());
    }

    fn windows_with_node(dir: &Path) -> Env {
        let node_dir = dir.join("nodejs");
        std::fs::create_dir_all(&node_dir).unwrap();
        std::fs::write(node_dir.join("node.exe"), "").unwrap();
        Env::with(Os::Windows, dir, &[("PATH", node_dir.to_str().unwrap())])
    }

    #[test]
    fn windows_installs_globally_and_points_node_at_the_bin() {
        let dir = tempfile::tempdir().unwrap();
        let env = windows_with_node(dir.path());
        let root = dir.path().join("npm").join("node_modules");
        let package = root.join("@roychri").join("mcp-server-asana");
        std::fs::create_dir_all(package.join("dist")).unwrap();
        std::fs::write(package.join("dist").join("index.js"), "").unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"bin":{"mcp-server-asana":"dist/index.js"}}"#,
        )
        .unwrap();
        let runner = FakeRunner::default().on("cmd", "install", ok("")).on(
            "cmd",
            "root",
            ok(&format!("{}\n", root.display())),
        );
        let packages = SystemPackages {
            env: &env,
            runner: &runner,
        };

        let server = packages
            .entry("@roychri/mcp-server-asana@beta", &["--stdio"])
            .unwrap();

        assert!(server.command.ends_with("node.exe"), "{}", server.command);
        assert_eq!(server.args.len(), 2);
        assert!(server.args[0].ends_with("index.js"));
        assert_eq!(server.args[1], "--stdio");
        assert_eq!(
            runner.calls()[0].args,
            [
                "/C",
                "npm",
                "install",
                "-g",
                "@roychri/mcp-server-asana@beta",
                "--no-audit",
                "--no-fund",
                "--no-progress"
            ]
        );
    }

    #[test]
    fn a_failed_install_reports_npms_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let env = windows_with_node(dir.path());
        let runner =
            FakeRunner::default().on("cmd", "install", fail(1, "a\nb\nc\nnpm ERR! offline\n"));
        let packages = SystemPackages {
            env: &env,
            runner: &runner,
        };
        let error = packages.entry("mcp-remote", &[]).unwrap_err();
        assert_eq!(
            error,
            "'npm install -g mcp-remote' failed (exit 1): b c npm ERR! offline"
        );
    }

    #[test]
    fn windows_without_node_says_so_before_installing() {
        let env = Env::with(Os::Windows, "/h", &[]);
        let runner = FakeRunner::default();
        let packages = SystemPackages {
            env: &env,
            runner: &runner,
        };
        assert!(
            packages
                .entry("mcp-remote", &[])
                .unwrap_err()
                .contains("Node.js")
        );
        assert!(runner.calls().is_empty());
    }
}
