//! The whole way on a real machine: set a server up as the app does, then
//! start it exactly as Claude Desktop would and ask it who it is.
//!
//! Ignored by default: it downloads an npm package (and on Windows installs
//! it globally) and needs Node.js.
//!
//!     cargo test -p agents-kitbag-core --test real_setup -- --ignored
//!
//! Azure DevOps is the one integration that needs no token, so nothing here
//! needs a secret. On Windows this is the path nothing else exercises for
//! real: `npm install -g`, finding the package's script, and a config that
//! starts `node.exe <script>` directly.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use agents_kitbag_core::config::ConfigFile;
use agents_kitbag_core::exec::SystemRunner;
use agents_kitbag_core::http::{Http, Request, Response};
use agents_kitbag_core::integrations;
use agents_kitbag_core::packages::SystemPackages;
use agents_kitbag_core::platform::{Env, Os};
use agents_kitbag_core::setup::{self, Ctx};
use agents_kitbag_core::storage::StoreChoice;

/// Azure DevOps asks no service anything.
struct NoHttp;

impl Http for NoHttp {
    fn send(&self, request: &Request) -> Result<Response, String> {
        Err(format!("unexpected request to {}", request.url))
    }
}

#[test]
#[ignore = "downloads an npm package and starts it"]
fn an_azure_devops_server_set_up_here_starts_and_answers() {
    let env = Env::current();
    let dir = tempfile::tempdir().unwrap();
    let config = ConfigFile::new(dir.path().join("claude_desktop_config.json"), env.os);
    let packages = SystemPackages {
        env: &env,
        runner: &SystemRunner,
    };
    let ctx = Ctx {
        env: &env,
        runner: &SystemRunner,
        http: &NoHttp,
        packages: &packages,
        config: &config,
        runner_path: "unused",
    };
    let request = setup::Request {
        integration: integrations::by_key("azure-devops").unwrap(),
        instance: Some("e2e".to_owned()),
        values: [(
            "org".to_owned(),
            "https://dev.azure.com/Contoso/Project".to_owned(),
        )]
        .into(),
        store: StoreChoice::Plain,
    };

    let done = setup::run(&ctx, &request).expect("setting Azure DevOps up");
    assert_eq!(done.key, "ado-e2e");
    assert_eq!(done.account, "organization: Contoso");

    let server = config
        .server("ado-e2e")
        .unwrap()
        .expect("the server is in the config");
    eprintln!("command: {} {:?}", server.command, server.args);
    assert_eq!(server.args.last().unwrap(), "Contoso");
    match env.os {
        // No shell and no npx at launch: Claude Desktop gives a server 60 s.
        Os::Windows => {
            assert!(
                server.command.to_lowercase().ends_with("node.exe"),
                "{}",
                server.command
            );
            assert!(
                std::path::Path::new(&server.args[0]).is_file(),
                "{}",
                server.args[0]
            );
        }
        Os::Mac | Os::Linux => assert_eq!(server.command, "npx"),
    }

    let mut child = Command::new(&server.command)
        .args(&server.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("starting the server as configured");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (lines, inbox) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "agents-kitbag-e2e", "version": "0" }
        }
    });
    writeln!(stdin, "{initialize}").unwrap();
    stdin.flush().unwrap();

    let started = std::time::Instant::now();
    let reply = loop {
        let line = inbox
            .recv_timeout(Duration::from_secs(180))
            .expect("an answer to initialize within three minutes");
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            panic!("stdout is the JSON-RPC transport, but the server printed: {line}");
        };
        if message["id"] == 1 {
            break message;
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    let name = reply["result"]["serverInfo"]["name"]
        .as_str()
        .unwrap_or_default();
    eprintln!(
        "answered in {:?}: {}",
        started.elapsed(),
        reply["result"]["serverInfo"]
    );
    assert!(name.contains("Azure DevOps"), "{reply}");
    if env.os == Os::Windows {
        // The reason for the global install: an `npx -y` start takes over a
        // minute cold on Windows, and Claude Desktop waits 60 s.
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "{:?}",
            started.elapsed()
        );
    }
}
