//! The MCP servers themselves are started with `npx` or `uvx`. Agents Kitbag
//! needs neither, but a server whose launcher is missing never starts.

use std::path::PathBuf;

use crate::integrations::Launcher;
use crate::platform::{Env, Os};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Missing {
    pub program: &'static str,
    pub needs: &'static str,
    pub url: &'static str,
}

/// The launcher, if it is in none of the places it is usually installed.
///
/// A warning, never a refusal: version managers put these programs in places
/// this list cannot know.
pub fn missing(env: &Env, launcher: Launcher) -> Option<Missing> {
    let (program, needs, url) = match launcher {
        Launcher::Npx => ("npx", "Node.js", "https://nodejs.org/"),
        Launcher::Uvx => (
            "uvx",
            "uv",
            "https://docs.astral.sh/uv/getting-started/installation/",
        ),
    };
    let found =
        env.on_path(program).is_some() || candidates(env, program).iter().any(|c| c.exists());
    (!found).then_some(Missing {
        program,
        needs,
        url,
    })
}

fn candidates(env: &Env, program: &str) -> Vec<PathBuf> {
    let home = &env.home;
    match env.os {
        Os::Mac | Os::Linux => {
            let mut places = vec![
                PathBuf::from("/opt/homebrew/bin").join(program),
                PathBuf::from("/usr/local/bin").join(program),
                PathBuf::from("/usr/bin").join(program),
                PathBuf::from("/snap/bin").join(program),
                PathBuf::from("/home/linuxbrew/.linuxbrew/bin").join(program),
                home.join(".local").join("bin").join(program),
                home.join(".cargo").join("bin").join(program),
                home.join(".volta").join("bin").join(program),
                home.join(".asdf").join("shims").join(program),
                home.join(".local")
                    .join("share")
                    .join("mise")
                    .join("shims")
                    .join(program),
            ];
            // nvm and fnm keep one folder per Node version.
            for versions in [
                home.join(".nvm").join("versions").join("node"),
                home.join(".local")
                    .join("share")
                    .join("fnm")
                    .join("node-versions"),
            ] {
                for version in std::fs::read_dir(versions).into_iter().flatten().flatten() {
                    places.push(version.path().join("bin").join(program));
                    places.push(
                        version
                            .path()
                            .join("installation")
                            .join("bin")
                            .join(program),
                    );
                }
            }
            places
        }
        Os::Windows => [
            env.path_under("ProgramFiles", &["nodejs", &format!("{program}.cmd")]),
            env.path_under("APPDATA", &["npm", &format!("{program}.cmd")]),
            env.path_under("USERPROFILE", &[".local", "bin", &format!("{program}.exe")]),
            env.path_under("USERPROFILE", &[".cargo", "bin", &format!("{program}.exe")]),
        ]
        .into_iter()
        .flatten()
        .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launcher_under_a_version_manager_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::with(Os::Mac, dir.path(), &[]);
        // Skipped on a machine with a system-wide npx: that one is found first.
        if candidates(&env, "npx").iter().any(|c| c.exists()) {
            return;
        }
        assert_eq!(missing(&env, Launcher::Npx).unwrap().needs, "Node.js");
        let bin = dir.path().join(".nvm/versions/node/v22.1.0/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("npx"), "").unwrap();
        assert_eq!(missing(&env, Launcher::Npx), None);
    }

    #[test]
    fn a_launcher_on_the_path_is_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("uvx.exe"), "").unwrap();
        let env = Env::with(
            Os::Windows,
            "/nowhere",
            &[("PATH", dir.path().to_str().unwrap())],
        );
        assert_eq!(missing(&env, Launcher::Uvx), None);
        let bare = Env::with(Os::Windows, "/nowhere", &[]);
        assert_eq!(missing(&bare, Launcher::Uvx).unwrap().program, "uvx");
    }
}
