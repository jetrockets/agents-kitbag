//! The whole way for the assistants that are command-line programs: write a
//! server into the assistant's config as the app does, its token in the
//! system's credential store, then have the real `claude` or `codex` start
//! it and see the token arrive.
//!
//! Ignored by default: they need Node.js, the assistant itself, and the
//! runner built beside the tests (`cargo build -p agents-kitbag-runner`).
//! The Codex one also needs Codex signed in, and makes one short request to
//! its model: Codex starts its servers when a session starts.
//!
//!     cargo build -p agents-kitbag-runner
//!     cargo test -p agents-kitbag-core --features net --test real_assistants -- --ignored --nocapture
//!
//! Each assistant is pointed at a folder of its own (`CLAUDE_CONFIG_DIR`,
//! `CODEX_HOME`), so the person's own config is not read or written. The
//! credential store entry is removed at the end.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    reason = "a test: its helpers fail the way its test functions do"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use agents_kitbag_core::assistant::Assistant;
use agents_kitbag_core::config::{ConfigFile, ServerConfig};
use agents_kitbag_core::exec::SystemRunner;
use agents_kitbag_core::platform::Env;
use agents_kitbag_core::runner;
use agents_kitbag_core::secret::Secret;
use agents_kitbag_core::storage::{self, StoreChoice};
use agents_kitbag_core::store::{SecretRef, Stores};

/// An MCP server that says where it was started and with what token, then
/// answers the handshake so the assistant counts it as connected.
const PROBE: &str = r#"
const fs = require("fs");
fs.writeFileSync(process.env.PROBE_OUT, process.env.PROBE_TOKEN || "(no token)");
let buffer = "";
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  let end;
  while ((end = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, end).trim();
    buffer = buffer.slice(end + 1);
    if (!line) continue;
    const message = JSON.parse(line);
    if (message.id === undefined) continue;
    let result = {};
    if (message.method === "initialize") {
      result = {
        protocolVersion: message.params.protocolVersion,
        capabilities: { tools: {} },
        serverInfo: { name: "probe", version: "0" },
      };
    } else if (message.method === "tools/list") {
      result = { tools: [] };
    }
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\n");
  }
});
"#;

fn runner_path() -> PathBuf {
    // target/debug/deps/real_assistants-<hash> -> target/debug/agents-kitbag-runner
    let path = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join(runner::NAME);
    assert!(
        path.is_file(),
        "build the runner first: cargo build -p agents-kitbag-runner ({} is missing)",
        path.display()
    );
    path
}

struct Probe {
    dir: tempfile::TempDir,
    /// Where the server writes the token it was started with.
    seen: PathBuf,
    token: String,
    reference: SecretRef,
}

/// Writes the probe into `assistant`'s config at `config_path` the way the
/// app writes any server with a stored token.
fn set_up(assistant: Assistant, config_path: impl FnOnce(&Path) -> PathBuf) -> Probe {
    let env = Env::current();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("probe.js");
    std::fs::write(&script, PROBE).unwrap();
    let seen = dir.path().join("seen.txt");
    let token = format!("probe-{}-{}", assistant.key(), std::process::id());
    let secret_key = assistant.secret_key(&format!("probe-test-{}", std::process::id()));

    let stored = storage::keep(
        &env,
        &SystemRunner,
        &runner_path().to_string_lossy(),
        &StoreChoice::System,
        &secret_key,
        &Secret::new(&token),
        None,
    )
    .expect("the system's credential store takes the token");
    let server = stored.apply(
        ServerConfig::new("node", [script.to_string_lossy().into_owned()]).with_env(&[
            ("PROBE_TOKEN", &stored.value),
            ("PROBE_OUT", &seen.to_string_lossy()),
        ]),
    );
    let reference = SecretRef::parse(server.args[1].split_once('=').unwrap().1).unwrap();

    let config = ConfigFile::of(assistant, config_path(dir.path()), env.os);
    config.set_server("probe", &server).unwrap();
    eprintln!(
        "{}:\n{}",
        config.path().display(),
        std::fs::read_to_string(config.path()).unwrap()
    );
    assert!(
        !std::fs::read_to_string(config.path())
            .unwrap()
            .contains(&token),
        "the token is in the config"
    );
    Probe {
        dir,
        seen,
        token,
        reference,
    }
}

impl Probe {
    /// What the server saw, once it has started.
    fn wait(&self, within: Duration) -> Option<String> {
        let started = Instant::now();
        while started.elapsed() < within {
            if let Ok(seen) = std::fs::read_to_string(&self.seen) {
                return Some(seen);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        None
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let env = Env::current();
        Stores {
            env: &env,
            runner: &SystemRunner,
        }
        .remove(&self.reference);
    }
}

fn run(command: &mut Command) -> Output {
    let output = command
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|error| panic!("{command:?} did not start: {error}"));
    eprintln!(
        "{command:?}\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
#[ignore = "needs Claude Code, Node.js and the built runner; writes one credential store entry"]
fn claude_code_starts_a_server_the_app_wrote_and_the_token_arrives() {
    let probe = set_up(Assistant::ClaudeCode, |dir| dir.join(".claude.json"));

    // `mcp get` starts the server to say whether it connects.
    let output = run(Command::new("claude")
        .args(["mcp", "get", "probe"])
        .env("CLAUDE_CONFIG_DIR", probe.dir.path())
        .current_dir(probe.dir.path()));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("User config"), "{text}");
    assert!(text.contains("Connected"), "{text}");
    assert_eq!(
        probe.wait(Duration::from_secs(5)).as_deref(),
        Some(probe.token.as_str())
    );
}

#[test]
#[ignore = "needs Codex signed in, Node.js and the built runner; makes one request to Codex's model"]
fn codex_starts_a_server_the_app_wrote_and_the_token_arrives() {
    let probe = set_up(Assistant::Codex, |dir| dir.join("config.toml"));

    // The sign-in, and nothing else, comes from the person's own Codex.
    let auth = Env::current().home.join(".codex").join("auth.json");
    assert!(
        auth.is_file(),
        "Codex is not signed in ({})",
        auth.display()
    );
    std::os::unix::fs::symlink(&auth, probe.dir.path().join("auth.json")).unwrap();

    let output = run(Command::new("codex")
        .args(["mcp", "get", "probe"])
        .env("CODEX_HOME", probe.dir.path()));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("transport: stdio"), "{text}");
    assert!(text.contains(runner::NAME), "{text}");

    // Codex starts its servers when a session starts.
    let mut session = Command::new("codex")
        .args([
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--sandbox",
            "read-only",
            "Reply with the single word: ok",
        ])
        .env("CODEX_HOME", probe.dir.path())
        .current_dir(probe.dir.path())
        .stdin(Stdio::null())
        .spawn()
        .expect("codex starts");
    let seen = probe.wait(Duration::from_secs(90));
    let _ = session.kill();
    let _ = session.wait();
    assert_eq!(seen.as_deref(), Some(probe.token.as_str()));
}
