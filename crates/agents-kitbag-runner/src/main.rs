//! Launches an MCP server with secrets fetched from the OS credential store.
//!
//! Claude Desktop passes an MCP server's `env` through verbatim, with no
//! variable expansion, so a secret can only reach the server through a
//! wrapper process. This is that wrapper, and it is a program rather than a
//! shell one-liner because Windows has no `sh -c`.
//!
//! ```text
//! agents-kitbag-runner --secret VAR=keychain:name [--secret ...] -- <command> [args...]
//! ```
//!
//! stdout belongs to the server's JSON-RPC transport: everything this
//! program prints goes to stderr, and nothing ever prints a secret.

use std::process::{Command, ExitCode};

use agents_kitbag_core::exec::SystemRunner;
use agents_kitbag_core::platform::Env;
use agents_kitbag_core::runner::{self, Plan};
use agents_kitbag_core::store::Stores;

/// sysexits.h: the command line was wrong.
const EX_USAGE: u8 = 64;
/// sysexits.h: a service this needs is not available.
const EX_UNAVAILABLE: u8 = 69;
/// The shell's "command not found".
const NOT_FOUND: u8 = 127;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().is_some_and(|a| a == "--version") {
        // stdout is free here: no server is started.
        println!("{} {}", runner::NAME, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    let plan = match runner::parse_args(&argv) {
        Ok(plan) => plan,
        Err(error) => return fail(&error, EX_USAGE),
    };
    let env = Env::current();
    let stores = Stores {
        env: &env,
        runner: &SystemRunner,
    };
    let secrets = match runner::resolve_env(&stores, &plan.secrets) {
        Ok(secrets) => secrets,
        Err(error) => return fail(&error, EX_UNAVAILABLE),
    };
    start(&plan, secrets)
}

fn fail(message: &str, code: u8) -> ExitCode {
    eprintln!("{}: {message}", runner::NAME);
    ExitCode::from(code)
}

fn command(plan: &Plan, secrets: Vec<(String, String)>) -> Command {
    let mut command = Command::new(&plan.command);
    command.args(&plan.args).envs(secrets);
    command
}

/// On Unix the runner becomes the server: same process id, same stdio, and
/// the signals Claude Desktop sends reach the server itself.
#[cfg(unix)]
fn start(plan: &Plan, secrets: Vec<(String, String)>) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let error = command(plan, secrets).exec();
    fail(
        &format!("cannot start {}: {error}", plan.command),
        NOT_FOUND,
    )
}

/// Windows cannot replace a process, so the runner waits and passes the
/// server's own exit status through: Claude sees what it would have seen
/// without the wrapper. The server inherits stdio and the console, so a
/// Ctrl+C or a closed pipe reaches it directly.
#[cfg(not(unix))]
fn start(plan: &Plan, secrets: Vec<(String, String)>) -> ExitCode {
    match command(plan, secrets).status() {
        Ok(status) => ExitCode::from(status.code().map_or(128, |code| code.clamp(0, 255) as u8)),
        Err(error) => fail(
            &format!("cannot start {}: {error}", plan.command),
            NOT_FOUND,
        ),
    }
}
