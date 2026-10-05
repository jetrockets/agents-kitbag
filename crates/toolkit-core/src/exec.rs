//! Every external command goes through here, so tests can answer for
//! `security`, `op`, `gh`, PowerShell and npm.

use std::io::Write;
use std::process::{Command, Stdio};

/// What a command answered. `code` is -1 when it could not be started.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

pub trait CommandRunner: Send + Sync {
    /// Runs `program` with `args`, writes `input` to its stdin, and waits.
    /// Secrets travel through `input`, never through `args`: command
    /// arguments are readable by any process on the machine.
    fn run(&self, program: &str, args: &[&str], input: Option<&str>) -> Output;
}

/// The real thing.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], input: Option<&str>) -> Output {
        let mut command = Command::new(program);
        command
            .args(args)
            // A pipe only when there is something to write. `op item create
            // --template` refuses to run with a pipe on stdin ("cannot
            // create an item from template and stdin at the same time"),
            // even an empty one that is closed at once.
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                log::debug!("{program} could not be started: {error}");
                return Output {
                    code: -1,
                    stdout: String::new(),
                    stderr: error.to_string(),
                };
            }
        };
        // Closing stdin (the drop) is what lets a command reading it finish.
        if let Some(mut stdin) = child.stdin.take()
            && let Some(input) = input
        {
            let _ = stdin.write_all(input.as_bytes());
        }
        match child.wait_with_output() {
            Ok(output) => {
                let code = output.status.code().unwrap_or(-1);
                // The program and how it ended, for `--verbose`. Not its
                // arguments (a PowerShell script is one), and never its
                // input or output: those are where secrets are.
                log::debug!("{program} exited {code}");
                Output {
                    code,
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                }
            }
            Err(error) => Output {
                code: -1,
                stdout: String::new(),
                stderr: error.to_string(),
            },
        }
    }
}

/// A windowed app that starts a console program gets a console window
/// flashing on screen for each one, unless it asks for none.
fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn input_reaches_stdin_and_the_exit_code_comes_back() {
        let out = SystemRunner.run("/bin/sh", &["-c", "cat; exit 3"], Some("hello"));
        assert_eq!(out.code, 3);
        assert_eq!(out.stdout, "hello");
    }

    #[test]
    fn without_input_stdin_is_not_a_pipe() {
        let script = "if [ -p /dev/stdin ]; then echo pipe; else echo none; fi; cat";
        let without = SystemRunner.run("/bin/sh", &["-c", script], None);
        assert_eq!(without.stdout, "none\n", "and reading it ends at once");
        let with = SystemRunner.run("/bin/sh", &["-c", script], Some("x"));
        assert_eq!(with.stdout, "pipe\nx");
    }

    #[test]
    fn a_program_that_is_not_there_is_a_failure_not_a_panic() {
        let out = SystemRunner.run("/no/such/program", &[], None);
        assert_eq!(out.code, -1);
        assert!(!out.ok());
    }
}
