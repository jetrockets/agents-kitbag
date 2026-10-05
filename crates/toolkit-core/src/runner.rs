//! The launcher Claude Desktop starts in place of an MCP server whose token
//! is in a credential store.
//!
//! Claude Desktop passes a server's `env` through verbatim, so a secret can
//! only reach the server through a wrapper process, and Windows has no
//! `sh -c` to be one.
//!
//! ```text
//! claude-toolkit-runner --secret VAR=keychain:name [--secret ...] -- <command> [args...]
//! ```
//!
//! This module is what both sides share: the arguments, and telling such a
//! server apart in the config. The program itself is `toolkit-runner`.

use crate::config::ServerConfig;
use crate::store::{SecretRef, Stores};

/// The Rust runner's file name, without `.exe`.
pub const NAME: &str = "claude-toolkit-runner";
/// The runner of the Node.js toolkit this app replaced, started as
/// `node .../secret-runner.js`. Configs it wrote are still out there.
pub const LEGACY_SCRIPT: &str = "secret-runner.js";

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub variable: String,
    pub reference: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub secrets: Vec<Binding>,
    pub command: String,
    pub args: Vec<String>,
}

/// The runner's own arguments (without the program name).
pub fn parse_args(argv: &[String]) -> Result<Plan, String> {
    let split = argv
        .iter()
        .position(|a| a == "--")
        .ok_or("Missing `--` separator before the command to run")?;
    let mut secrets = Vec::new();
    let mut before = argv[..split].iter();
    while let Some(arg) = before.next() {
        if arg != "--secret" {
            continue;
        }
        let binding = before.next().map(String::as_str).unwrap_or_default();
        match binding.split_once('=') {
            Some((variable, reference)) if !variable.is_empty() => secrets.push(Binding {
                variable: variable.to_owned(),
                reference: reference.to_owned(),
            }),
            _ => {
                return Err(format!(
                    "--secret expects VAR=<backend>:<name>, got \"{binding}\""
                ));
            }
        }
    }
    let mut rest = argv[split + 1..].iter().cloned();
    let command = rest.next().ok_or("No command given after `--`")?;
    Ok(Plan {
        secrets,
        command,
        args: rest.collect(),
    })
}

/// The variables to start the server with. An error names the variable and
/// the reference, never a value.
pub fn resolve_env(stores: &Stores, secrets: &[Binding]) -> Result<Vec<(String, String)>, String> {
    secrets
        .iter()
        .map(|binding| {
            let reference = SecretRef::parse(&binding.reference)?;
            match stores.read(&reference) {
                Some(secret) => Ok((binding.variable.clone(), secret.expose().to_owned())),
                None => Err(format!(
                    "Could not read {} from {}: {}",
                    binding.variable,
                    binding.reference,
                    reason(stores, &reference)
                )),
            }
        })
        .collect()
}

/// "The entry is missing" is misleading when the backend command itself is
/// absent, which is what happens with a bare system PATH.
fn reason(stores: &Stores, reference: &SecretRef) -> String {
    if stores.usable(reference.backend) {
        "the entry is missing or the store is locked".to_owned()
    } else {
        format!("{} is not available here", reference.backend.label())
    }
}

/// Which runner, if any, launches this server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `node .../secret-runner.js`, written by the Node.js toolkit.
    Legacy,
    /// `claude-toolkit-runner`.
    Native,
}

pub fn kind(server: &ServerConfig) -> Option<Kind> {
    let bin = server
        .command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if bin == NAME || bin == format!("{NAME}.exe") {
        return Some(Kind::Native);
    }
    before_separator(server)
        .iter()
        .any(|arg| arg.ends_with(LEGACY_SCRIPT))
        .then_some(Kind::Legacy)
}

fn before_separator(server: &ServerConfig) -> &[String] {
    let stop = server
        .args
        .iter()
        .position(|a| a == "--")
        .unwrap_or(server.args.len());
    &server.args[..stop]
}

/// The reference bound to `variable` on a runner's command line, or the first
/// binding when no variable is named.
pub fn binding(server: &ServerConfig, variable: Option<&str>) -> Option<String> {
    kind(server)?;
    let args = before_separator(server);
    args.iter()
        .enumerate()
        .filter(|(_, arg)| *arg == "--secret")
        .filter_map(|(i, _)| args.get(i + 1)?.split_once('='))
        .find(|(name, _)| variable.is_none_or(|v| v == *name))
        .map(|(_, reference)| reference.to_owned())
        .filter(|reference| !reference.is_empty())
}

/// The server's own command and arguments, after the runner's `--`.
pub fn inner_command(server: &ServerConfig) -> Option<(&str, &[String])> {
    kind(server)?;
    let split = server.args.iter().position(|a| a == "--")?;
    let (command, args) = server.args[split + 1..].split_first()?;
    Some((command, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Env, Os};
    use crate::testing::{FakeRunner, fail, ok};

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn secrets_and_the_command_are_split_at_the_separator() {
        let plan = parse_args(&argv(&[
            "--secret",
            "A=keychain:a",
            "--secret",
            "B=gh:",
            "--",
            "npx",
            "-y",
            "pkg",
            "--secret",
            "C=x",
        ]))
        .unwrap();
        assert_eq!(
            plan.secrets,
            [
                Binding {
                    variable: "A".into(),
                    reference: "keychain:a".into()
                },
                Binding {
                    variable: "B".into(),
                    reference: "gh:".into()
                },
            ]
        );
        assert_eq!(plan.command, "npx");
        assert_eq!(plan.args, ["-y", "pkg", "--secret", "C=x"]);
    }

    #[test]
    fn a_reference_may_contain_an_equals_sign() {
        let plan = parse_args(&argv(&["--secret", "A=keychain:a=b", "--", "x"])).unwrap();
        assert_eq!(plan.secrets[0].reference, "keychain:a=b");
    }

    #[test]
    fn malformed_arguments_are_refused() {
        assert!(
            parse_args(&argv(&["npx"]))
                .unwrap_err()
                .contains("separator")
        );
        assert!(
            parse_args(&argv(&["--"]))
                .unwrap_err()
                .contains("No command")
        );
        assert!(parse_args(&argv(&["--secret", "=x", "--", "y"])).is_err());
        assert!(parse_args(&argv(&["--secret", "novalue", "--", "y"])).is_err());
        assert!(parse_args(&argv(&["--secret", "--", "y"])).is_err());
    }

    #[test]
    fn the_environment_is_read_from_the_stores() {
        let runner = FakeRunner::default().on("security", "find-generic-password", ok("tok\n"));
        let env = Env::with(Os::Mac, "/h", &[("USER", "me")]);
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        let resolved = resolve_env(
            &stores,
            &[Binding {
                variable: "T".into(),
                reference: "keychain:claude-mcp-x".into(),
            }],
        )
        .unwrap();
        assert_eq!(resolved, [("T".to_owned(), "tok".to_owned())]);
    }

    #[test]
    fn a_secret_that_cannot_be_read_says_why() {
        let env = Env::with(Os::Mac, "/h", &[]);
        let binding = [Binding {
            variable: "T".into(),
            reference: "keychain:x".into(),
        }];

        let missing = FakeRunner::default()
            .on("security", "find-generic-password", fail(44, ""))
            .on("security", "default-keychain", ok(""));
        let error = resolve_env(
            &Stores {
                env: &env,
                runner: &missing,
            },
            &binding,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "Could not read T from keychain:x: the entry is missing or the store is locked"
        );

        let absent = FakeRunner::default();
        let error = resolve_env(
            &Stores {
                env: &env,
                runner: &absent,
            },
            &binding,
        )
        .unwrap_err();
        assert!(
            error.ends_with("macOS Keychain is not available here"),
            "{error}"
        );
    }

    fn legacy() -> ServerConfig {
        ServerConfig::new(
            "/usr/local/bin/node",
            [
                "/path/to/claude-toolkit/src/secret-runner.js",
                "--secret",
                "ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana",
                "--",
                "npx",
                "-y",
                "@roychri/mcp-server-asana@beta",
            ],
        )
    }

    fn native() -> ServerConfig {
        ServerConfig::new(
            r"C:\Apps\Claude Toolkit\claude-toolkit-runner.exe",
            [
                "--secret",
                "A=dpapi:a",
                "--secret",
                "B=gh:",
                "--",
                "node",
                "server.js",
            ],
        )
    }

    #[test]
    fn both_runners_are_recognised() {
        assert_eq!(kind(&legacy()), Some(Kind::Legacy));
        assert_eq!(kind(&native()), Some(Kind::Native));
        assert_eq!(kind(&ServerConfig::new("npx", ["-y", "pkg"])), None);
        // A server that merely mentions the script after `--` is not one.
        let mention = ServerConfig::new("node", ["x.js", "--", "secret-runner.js"]);
        assert_eq!(kind(&mention), None);
    }

    #[test]
    fn bindings_are_found_by_variable_or_first() {
        assert_eq!(
            binding(&legacy(), None).as_deref(),
            Some("keychain:claude-mcp-asana")
        );
        assert_eq!(binding(&native(), Some("B")).as_deref(), Some("gh:"));
        assert_eq!(binding(&native(), Some("A")).as_deref(), Some("dpapi:a"));
        assert_eq!(binding(&native(), Some("C")), None);
        assert_eq!(
            binding(&ServerConfig::new("npx", ["--secret", "A=b"]), None),
            None
        );
    }

    #[test]
    fn the_inner_command_is_what_follows_the_separator() {
        let server = legacy();
        let (command, args) = inner_command(&server).unwrap();
        assert_eq!(command, "npx");
        assert_eq!(args, ["-y", "@roychri/mcp-server-asana@beta"]);
    }
}
