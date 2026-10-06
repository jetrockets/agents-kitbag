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
/// Ctrl+C or a closed pipe reaches it directly. An assistant that ends the
/// runner outright ends the server with it, through a job object.
#[cfg(not(unix))]
fn start(plan: &Plan, secrets: Vec<(String, String)>) -> ExitCode {
    let mut child = match command(plan, secrets).spawn() {
        Ok(child) => child,
        Err(error) => {
            return fail(
                &format!("cannot start {}: {error}", plan.command),
                NOT_FOUND,
            );
        }
    };
    #[cfg(windows)]
    job::end_with_the_runner(&child);
    match child.wait() {
        Ok(status) => ExitCode::from(status.code().map_or(128, |code| code.clamp(0, 255) as u8)),
        Err(error) => fail(&format!("lost {}: {error}", plan.command), NOT_FOUND),
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// Puts the server in a job that Windows closes when the runner goes,
    /// however it goes, and that takes the server (and what it started
    /// from then on) with it. Without it a runner that is terminated leaves
    /// `node` running with nobody to talk to.
    ///
    /// Best effort: a server that cannot be put in a job still runs.
    #[allow(
        unsafe_code,
        reason = "the job object API is only reachable through the Windows bindings"
    )]
    pub fn end_with_the_runner(child: &Child) {
        // SAFETY: the calls take a null name and security descriptor, a
        // zeroed and fully sized information block that outlives the call,
        // and the child's own live handle. The job handle is never closed on
        // purpose: the runner's exit closes it, which is what ends the job.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set != 0 {
                AssignProcessToJobObject(job, child.as_raw_handle().cast());
            }
        }
    }
}
