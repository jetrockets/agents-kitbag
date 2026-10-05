//! The built runner, started the way Claude Desktop starts it.

use std::process::Command;

fn runner() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agents-kitbag-runner"))
}

#[test]
fn version_names_the_program() {
    let output = runner().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("agents-kitbag-runner {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn a_command_line_without_a_separator_is_a_usage_error() {
    let output = runner().args(["npx", "-y", "pkg"]).output().unwrap();
    assert_eq!(output.status.code(), Some(64));
    assert!(output.stdout.is_empty(), "stdout is the server's");
    assert!(String::from_utf8_lossy(&output.stderr).contains("separator"));
}

#[test]
fn an_unknown_backend_is_unavailable_and_starts_nothing() {
    let output = runner()
        .args(["--secret", "T=vault:thing", "--", "/bin/echo", "started"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(69));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown backend"));
}

#[cfg(unix)]
#[test]
fn the_server_runs_with_its_arguments_stdio_and_exit_code() {
    let output = runner()
        .args([
            "--",
            "/bin/sh",
            "-c",
            "echo \"$1\"; exit 7",
            "sh",
            "hello world",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hello world\n");
}

#[cfg(unix)]
#[test]
fn a_command_that_is_not_there_exits_127() {
    let output = runner().args(["--", "/no/such/server"]).output().unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot start"));
}

#[cfg(windows)]
#[test]
fn on_windows_the_server_runs_with_its_arguments_stdio_and_exit_code() {
    let output = runner()
        .args(["--", "cmd", "/C", "echo hello world& exit 7"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello world"
    );
}

/// The runner stays the server's parent on Windows; ending the runner must
/// end the server too.
#[cfg(windows)]
#[test]
fn on_windows_a_runner_that_is_ended_takes_the_server_with_it() {
    use std::time::{Duration, Instant};

    let dir = std::env::temp_dir().join(format!("agents-kitbag-job-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pid_file = dir.join("server.pid");
    let script = format!(
        "$PID | Out-File -Encoding ascii '{}'; Start-Sleep -Seconds 120",
        pid_file.display()
    );
    let mut runner = runner()
        .args(["--", "powershell", "-NoProfile", "-Command", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let started = Instant::now();
    let pid = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file)
            && let Ok(pid) = text.trim().parse::<u32>()
        {
            break pid;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the server never started"
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    let running = || {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
    };
    assert!(
        running(),
        "the server is running before the runner is ended"
    );

    runner.kill().unwrap();
    runner.wait().unwrap();
    let ended = Instant::now();
    while running() && ended.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(200));
    }
    let left = running();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!left, "the server outlived the runner");
}

#[cfg(windows)]
#[test]
fn on_windows_a_command_that_is_not_there_exits_127() {
    let output = runner()
        .args(["--", r"C:\no\such\server.exe"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot start"));
}

/// A secret in this system's own store, read by the built runner and handed
/// to the server it starts. Leaves nothing behind.
///
/// Ignored: it writes to the login store. CI runs it on macOS and Windows.
///
///     cargo test -p agents-kitbag-runner --test cli a_native_secret_reaches_the_server -- --ignored --exact
#[test]
#[ignore = "writes to the real credential store"]
fn a_native_secret_reaches_the_server() {
    use agents_kitbag_core::exec::SystemRunner;
    use agents_kitbag_core::platform::Env;
    use agents_kitbag_core::secret::Secret;
    use agents_kitbag_core::store::{Backend, SecretRef, Stores};

    let env = Env::current();
    let stores = Stores {
        env: &env,
        runner: &SystemRunner,
    };
    let reference = SecretRef {
        backend: Backend::system(env.os),
        name: format!("agents-kitbag-runner-test-{}", std::process::id()),
    };
    stores
        .store(&reference, &Secret::new("from-the-store"))
        .expect("storing");
    let print: &[&str] = if cfg!(windows) {
        &["cmd", "/C", "echo %AGENTS_KITBAG_TEST_TOKEN%"]
    } else {
        &["/bin/sh", "-c", "printf %s \"$AGENTS_KITBAG_TEST_TOKEN\""]
    };
    let output = runner()
        .args([
            "--secret",
            &format!("AGENTS_KITBAG_TEST_TOKEN={reference}"),
            "--",
        ])
        .args(print)
        .output();
    stores.remove(&reference);
    let output = output.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "from-the-store"
    );

    // Removed means gone: nothing is started, and stdout stays the server's.
    let output = runner()
        .args([
            "--secret",
            &format!("AGENTS_KITBAG_TEST_TOKEN={reference}"),
            "--",
        ])
        .args(print)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(69));
    assert!(output.stdout.is_empty());
}
