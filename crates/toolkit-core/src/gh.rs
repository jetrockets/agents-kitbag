//! The GitHub CLI: GitHub's integration uses its token instead of asking for one.

use crate::exec::CommandRunner;
use crate::platform::{Env, Os};
use crate::secret::Secret;
use crate::store::resolve_bin;

pub struct Gh<'a> {
    pub env: &'a Env,
    pub runner: &'a dyn CommandRunner,
}

/// What stands between the person and a GitHub server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    NotSignedIn,
    Ready,
}

impl Gh<'_> {
    pub fn bin(&self) -> String {
        resolve_bin(self.env, "gh")
    }

    pub fn state(&self) -> State {
        if !self.runner.run(&self.bin(), &["--version"], None).ok() {
            State::NotInstalled
        } else if self.runner.run(&self.bin(), &["auth", "status"], None).ok() {
            State::Ready
        } else {
            State::NotSignedIn
        }
    }

    pub fn token(&self) -> Option<Secret> {
        let output = self.runner.run(&self.bin(), &["auth", "token"], None);
        let token = output.stdout.trim();
        (output.ok() && !token.is_empty()).then(|| Secret::new(token))
    }

    /// The scopes the CLI's token carries, read off `gh auth status`.
    pub fn scopes(&self) -> Vec<String> {
        let output = self.runner.run(&self.bin(), &["auth", "status"], None);
        parse_scopes(&format!("{}{}", output.stdout, output.stderr))
    }

    /// The server filters tools by OAuth scope: without `project` (read and
    /// write) the Projects tools stay hidden, and `gh auth login`'s default
    /// scopes do not include it. `read:project` alone would be read-only.
    pub fn has_project_scope(&self) -> bool {
        self.scopes().iter().any(|scope| scope == "project")
    }

    /// Installs the CLI with the platform's package manager.
    pub fn install(&self) -> Result<(), String> {
        let output = match self.env.os {
            Os::Mac => {
                let brew = Env::first_existing(
                    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].map(Into::into),
                )
                .ok_or("Homebrew was not found. Install gh from https://cli.github.com/")?;
                self.runner
                    .run(&brew.to_string_lossy(), &["install", "gh"], None)
            }
            Os::Linux => {
                return Err(
                    "Install the GitHub CLI with your package manager: https://cli.github.com/"
                        .to_owned(),
                );
            }
            Os::Windows => self.runner.run(
                "winget",
                &[
                    "install",
                    "GitHub.cli",
                    "--silent",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ],
                None,
            ),
        };
        if output.ok() {
            return Ok(());
        }
        let last = output.stderr.trim().lines().last().unwrap_or_default();
        Err(format!(
            "Could not install the GitHub CLI ({last}). Install it from https://cli.github.com/"
        ))
    }

    /// `gh auth login`, for a terminal: it asks questions and shows a code.
    pub fn login_command(&self) -> String {
        format!("{} auth login", quoted(&self.bin()))
    }

    /// Adds the `project` scope to the CLI's token, for a terminal.
    pub fn add_project_scope_command(&self) -> String {
        format!(
            "{} auth refresh -h github.com -s project",
            quoted(&self.bin())
        )
    }
}

fn quoted(path: &str) -> String {
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path.to_owned()
    }
}

fn parse_scopes(status: &str) -> Vec<String> {
    let Some((_, rest)) = status.split_once("Token scopes:") else {
        return Vec::new();
    };
    rest.lines()
        .next()
        .unwrap_or_default()
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Runs a command in a terminal window the person can answer in.
pub fn open_in_terminal(os: Os, command: &str) -> Result<(), String> {
    let spawned = match os {
        Os::Mac => {
            let script = format!(
                "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
                command.replace('\\', "\\\\").replace('"', "\\\"")
            );
            std::process::Command::new("/usr/bin/osascript")
                .args(["-e", &script])
                .spawn()
        }
        Os::Windows => windows_terminal(command),
        Os::Linux => linux_terminal(command),
    };
    spawned.map(drop).map_err(|e| e.to_string())
}

/// The first terminal emulator that is installed. The window stays open
/// after the command, so what it printed can be read.
fn linux_terminal(command: &str) -> std::io::Result<std::process::Child> {
    let script = format!("{command}; echo; echo 'You can close this window.'; exec sh");
    let emulators: [(&str, &[&str]); 6] = [
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("kitty", &[]),
        ("xterm", &["-e"]),
    ];
    let mut last = std::io::Error::other("no terminal emulator was found");
    for (program, flags) in emulators {
        match std::process::Command::new(program)
            .args(flags)
            .args(["sh", "-c", &script])
            .spawn()
        {
            Ok(child) => return Ok(child),
            Err(error) => last = error,
        }
    }
    Err(last)
}

#[cfg(windows)]
fn windows_terminal(command: &str) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    // cmd's own quoting rules, which Rust's argument escaping would break:
    // the whole command line after /K is taken as written.
    std::process::Command::new("cmd")
        .raw_arg(format!("/C start \"GitHub CLI\" cmd /K \"{command}\""))
        .spawn()
}

#[cfg(not(windows))]
fn windows_terminal(_: &str) -> std::io::Result<std::process::Child> {
    Err(std::io::Error::other("not Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, fail, ok};

    fn env() -> Env {
        Env::with(Os::Mac, "/h", &[])
    }

    #[test]
    fn scopes_are_read_from_the_status_line() {
        let status = "github.com\n  ✓ Logged in to github.com account me (keyring)\n  - Token scopes: 'gist', 'project', 'read:org', 'repo'\n";
        assert_eq!(
            parse_scopes(status),
            ["gist", "project", "read:org", "repo"]
        );
        assert!(parse_scopes("not logged in").is_empty());
    }

    #[test]
    fn the_state_tells_missing_from_signed_out() {
        let env = env();
        let missing = FakeRunner::default();
        assert_eq!(
            Gh {
                env: &env,
                runner: &missing
            }
            .state(),
            State::NotInstalled
        );

        let signed_out = FakeRunner::default()
            .on("gh", "--version", ok("gh version 2"))
            .on(
                "gh",
                "status",
                fail(1, "You are not logged into any GitHub hosts"),
            );
        assert_eq!(
            Gh {
                env: &env,
                runner: &signed_out
            }
            .state(),
            State::NotSignedIn
        );

        let ready = FakeRunner::default()
            .on("gh", "--version", ok("gh version 2"))
            .on("gh", "status", ok("Token scopes: 'repo'"));
        let gh = Gh {
            env: &env,
            runner: &ready,
        };
        assert_eq!(gh.state(), State::Ready);
        assert!(!gh.has_project_scope());
    }

    #[test]
    fn a_path_with_spaces_is_quoted_for_the_terminal() {
        assert_eq!(
            quoted(r"C:\Program Files\GitHub CLI\gh.exe"),
            "\"C:\\Program Files\\GitHub CLI\\gh.exe\""
        );
        assert_eq!(quoted("/opt/homebrew/bin/gh"), "/opt/homebrew/bin/gh");
    }
}
