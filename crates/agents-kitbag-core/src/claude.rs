//! Claude Desktop reads its config when it starts, so a change needs a restart.

use std::time::Duration;

use crate::exec::CommandRunner;
use crate::platform::Os;

pub fn is_running(runner: &dyn CommandRunner, os: Os) -> bool {
    match os {
        Os::Windows => runner
            .run("tasklist", &["/FI", "IMAGENAME eq Claude.exe", "/NH"], None)
            .stdout
            .contains("Claude.exe"),
        Os::Mac => runner.run("/usr/bin/pgrep", &["-x", "Claude"], None).ok(),
        // The community builds do not agree on a process name.
        Os::Linux => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart {
    Restarted,
    /// Nothing to restart: the servers load the next time Claude opens.
    NotRunning,
    /// It was asked to quit and is still there: opening it again would only
    /// bring the old process forward, with the old config.
    DidNotQuit,
    /// Windows and Linux: the person quits and reopens Claude Desktop.
    ///
    /// On Linux there is no official build to know how to start. On Windows:
    ///
    /// A forced kill and relaunch loses the MSIX packaged-app context
    /// (environment, working folder) and sometimes leaves the app half
    /// started; on some machines the relaunch resolves to a stale shortcut
    /// and the app does not come back. A reliable restart needs the package
    /// family name and `explorer.exe shell:AppsFolder\<name>!App`.
    Manual,
}

/// The steps shown for [`Restart::Manual`].
pub const MANUAL_STEPS: [&str; 3] = [
    "Fully quit Claude Desktop (its tray icon, Quit).",
    "Open Claude Desktop again.",
    "Your MCP servers are available in new chats.",
];

pub fn restart(runner: &dyn CommandRunner, os: Os) -> Restart {
    restart_waiting(runner, os, Duration::from_millis(500))
}

fn restart_waiting(runner: &dyn CommandRunner, os: Os, pause: Duration) -> Restart {
    if os != Os::Mac {
        return Restart::Manual;
    }
    if !is_running(runner, os) {
        return Restart::NotRunning;
    }
    runner.run("/usr/bin/pkill", &["-x", "Claude"], None);
    for _ in 0..20 {
        if !is_running(runner, os) {
            break;
        }
        std::thread::sleep(pause);
    }
    if is_running(runner, os) {
        return Restart::DidNotQuit;
    }
    runner.run("/usr/bin/open", &["-a", "Claude"], None);
    Restart::Restarted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, fail, ok};

    #[test]
    fn windows_and_linux_are_restarted_by_hand() {
        let runner = FakeRunner::default();
        assert_eq!(restart(&runner, Os::Windows), Restart::Manual);
        assert_eq!(restart(&runner, Os::Linux), Restart::Manual);
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn a_claude_that_is_not_running_is_left_alone() {
        let runner = FakeRunner::default().on("pgrep", "Claude", fail(1, ""));
        assert_eq!(restart(&runner, Os::Mac), Restart::NotRunning);
        assert_eq!(runner.calls().len(), 1);
    }

    /// A Claude that is running until it is told to quit, or for good.
    struct Claude {
        quits: bool,
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl Claude {
        fn new(quits: bool) -> Self {
            Self {
                quits,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandRunner for Claude {
        fn run(&self, program: &str, _: &[&str], _: Option<&str>) -> crate::exec::Output {
            let mut calls = self.calls.lock().unwrap();
            let killed = calls.iter().any(|call| call.ends_with("pkill"));
            calls.push(program.to_owned());
            if program.ends_with("pgrep") && killed && self.quits {
                fail(1, "")
            } else {
                ok("123")
            }
        }
    }

    #[test]
    fn a_running_claude_is_quit_and_opened_again() {
        let runner = Claude::new(true);
        assert_eq!(
            restart_waiting(&runner, Os::Mac, Duration::ZERO),
            Restart::Restarted
        );
        let programs = runner.calls();
        assert_eq!(programs[1], "/usr/bin/pkill");
        assert_eq!(programs.last().unwrap(), "/usr/bin/open");
    }

    #[test]
    fn a_claude_that_will_not_quit_is_not_called_restarted() {
        let runner = Claude::new(false);
        assert_eq!(
            restart_waiting(&runner, Os::Mac, Duration::ZERO),
            Restart::DidNotQuit
        );
        assert!(!runner.calls().iter().any(|call| call.ends_with("open")));
    }

    #[test]
    fn tasklist_names_a_running_claude_on_windows() {
        let running =
            FakeRunner::default().on("tasklist", "Claude.exe", ok("Claude.exe  1234 Console"));
        assert!(is_running(&running, Os::Windows));
        let stopped =
            FakeRunner::default().on("tasklist", "Claude.exe", ok("INFO: No tasks are running"));
        assert!(!is_running(&stopped, Os::Windows));
    }
}
