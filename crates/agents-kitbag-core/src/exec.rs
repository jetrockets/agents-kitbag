//! Every external command goes through here, so tests can answer for
//! `security`, `op`, `gh`, PowerShell and npm.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

    /// A command that could not be started, or was given up on.
    fn failed(stderr: String) -> Self {
        Self {
            code: -1,
            stdout: String::new(),
            stderr,
        }
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
        run_within(program, args, input, limit(program))
    }
}

/// Runs the command, and gives up on it after `limit`.
fn run_within(program: &str, args: &[&str], input: Option<&str>, limit: Duration) -> Output {
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
            return Output::failed(error.to_string());
        }
    };
    // Closing stdin (the drop) is what lets a command reading it finish.
    if let Some(mut stdin) = child.stdin.take()
        && let Some(input) = input
    {
        let _ = stdin.write_all(input.as_bytes());
    }
    // Read both streams on their own threads, so a command that fills
    // one pipe cannot stall, and so the wait below can give up.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limit => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                // A prompt nobody answers (1Password waiting to be
                // unlocked, a Keychain dialog) would hold the one worker
                // thread, and everything queued behind it, for good.
                let _ = child.kill();
                let _ = child.wait();
                log::warn!("{program} did not answer in {} s", limit.as_secs());
                return Output::failed(format!(
                    "{} did not answer in {} seconds",
                    program_name(program),
                    limit.as_secs()
                ));
            }
            Err(error) => return Output::failed(error.to_string()),
        }
    };
    let collect = |reader: Option<std::thread::JoinHandle<Vec<u8>>>| {
        reader
            .and_then(|reader| reader.join().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    let code = status.code().unwrap_or(-1);
    // The program and how it ended, for `--verbose`. Not its arguments (a
    // PowerShell script is one), and never its input or output: those are
    // where secrets are.
    log::debug!("{program} exited {code}");
    Output {
        code,
        stdout: collect(stdout),
        stderr: collect(stderr),
    }
}

fn drain(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        bytes
    })
}

/// The program's file name in lowercase, without a folder or `.exe`.
pub fn program_name(program: &str) -> String {
    let file = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    match file.strip_suffix(".exe") {
        Some(name) => name.to_owned(),
        None => file,
    }
}

/// How long a command may take. Installing a package is slow on a cold
/// cache; everything else answers at once unless it is waiting for a person,
/// who gets two minutes to unlock 1Password or allow the Keychain.
fn limit(program: &str) -> Duration {
    match program_name(program).as_str() {
        "npm" | "npx" | "node" | "uvx" | "uv" | "brew" | "winget" | "cmd" => {
            Duration::from_secs(15 * 60)
        }
        _ => Duration::from_secs(120),
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
    fn a_command_that_never_answers_is_given_up_on() {
        let started = Instant::now();
        let out = run_within(
            "/bin/sh",
            &["-c", "echo early; sleep 30"],
            None,
            Duration::from_secs(1),
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(out.code, -1);
        assert_eq!(out.stderr, "sh did not answer in 1 seconds");
    }

    #[test]
    fn a_lot_of_output_on_both_streams_does_not_stall() {
        let script =
            "head -c 300000 /dev/zero | tr '\\0' x; head -c 300000 /dev/zero | tr '\\0' y >&2";
        let out = SystemRunner.run("/bin/sh", &["-c", script], None);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout.len(), 300_000);
        assert_eq!(out.stderr.len(), 300_000);
    }

    #[test]
    fn installers_get_longer_than_everything_else() {
        assert_eq!(limit("/usr/bin/security"), Duration::from_secs(120));
        assert_eq!(
            limit(r"C:\Program Files\nodejs\npm.exe"),
            Duration::from_secs(900)
        );
        assert_eq!(limit("cmd"), Duration::from_secs(900));
    }

    #[test]
    fn a_program_that_is_not_there_is_a_failure_not_a_panic() {
        let out = SystemRunner.run("/no/such/program", &[], None);
        assert_eq!(out.code, -1);
        assert!(!out.ok());
    }
}
