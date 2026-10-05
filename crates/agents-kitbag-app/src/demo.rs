//! `--demo`: made-up integrations in a throwaway folder. Nothing is sent,
//! stored or started; every command and request is answered here.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use agents_kitbag_core::config::{ConfigFile, ServerConfig};
use agents_kitbag_core::exec::{CommandRunner, Output};
use agents_kitbag_core::http::{Http, Request, Response};
use agents_kitbag_core::op;
use agents_kitbag_core::platform::{Env, Os};

use crate::backend::Backend;

/// The one token the made-up services refuse.
pub const BAD_TOKEN: &str = "expired";

const RUNNER: &str = "/Applications/Agents Kitbag.app/Contents/MacOS/agents-kitbag-runner";

fn ok(stdout: &str) -> Output {
    Output {
        code: 0,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

fn fail() -> Output {
    Output {
        code: 1,
        stdout: String::new(),
        stderr: "not in the demo".to_owned(),
    }
}

/// Answers for `security`, `gh`, `op` and the process tools.
#[derive(Default)]
struct DemoRunner {
    claude_quit: AtomicBool,
}

impl CommandRunner for DemoRunner {
    fn run(&self, program: &str, args: &[&str], _input: Option<&str>) -> Output {
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let has = |arg: &str| args.contains(&arg);
        match name {
            "security" if has("find-generic-password") => ok("demo-token\n"),
            "security" => ok(""),
            "gh" if has("--version") => ok("gh version 2.60.0\n"),
            "gh" if has("status") => {
                ok("github.com\n  - Token scopes: 'gist', 'project', 'read:org', 'repo'\n")
            }
            "gh" if has("token") => ok("gho_demo\n"),
            "op" if has("vault") => ok(r#"[{"name":"Private"},{"name":"Work"}]"#),
            "op" if has("create") => ok("{}"),
            "op" if has("read") => ok("demo-token\n"),
            "pgrep" if self.claude_quit.load(Ordering::Relaxed) => fail(),
            "pgrep" => ok("4242\n"),
            "pkill" => {
                self.claude_quit.store(true, Ordering::Relaxed);
                ok("")
            }
            "open" => ok(""),
            _ => fail(),
        }
    }
}

/// Every service answers 200 with a made-up account, except for
/// [`BAD_TOKEN`], which gets the 401 an expired token would.
struct DemoHttp;

impl Http for DemoHttp {
    fn send(&self, request: &Request) -> Result<Response, String> {
        let refused = request
            .headers
            .iter()
            .any(|(_, value)| value.contains(BAD_TOKEN));
        if refused {
            return Ok(Response {
                status: 401,
                body: "{}".to_owned(),
            });
        }
        let body = match &request.url {
            url if url.contains("atlassian") => r#"{"displayName":"Ada Lovelace"}"#,
            url if url.contains("linear") => {
                r#"{"data":{"viewer":{"id":"1","name":"Ada Lovelace","email":"ada@acme.com"}}}"#
            }
            url if url.contains("notion") => r#"{"name":"Claude MCP"}"#,
            url if url.contains("asana") => r#"{"data":{"name":"Ada Lovelace"}}"#,
            url if url.contains("github") => r#"{"login":"ada"}"#,
            url if url.contains("figma") => r#"{"handle":"Ada Lovelace"}"#,
            _ => "{}",
        };
        Ok(Response {
            status: 200,
            body: body.to_owned(),
        })
    }
}

/// A backend over a config in `folder`, holding one server of each kind of
/// storage: the Keychain, the Node runner, the GitHub CLI and plain text
/// (with a token the service refuses).
pub fn backend(folder: &Path) -> Backend {
    let env = Env::with(
        Os::Mac,
        folder,
        &[("USER", "ada"), (op::PATH_OVERRIDE, "/opt/homebrew/bin/op")],
    );
    let config = ConfigFile::new(folder.join("claude_desktop_config.json"), Os::Mac);
    let servers = [
        (
            "jira-acme",
            ServerConfig::new(
                RUNNER,
                [
                    "--secret",
                    "JIRA_API_TOKEN=keychain:agents-kitbag-jira-acme",
                    "--",
                    "uvx",
                    "mcp-atlassian",
                ],
            )
            .with_env(&[
                ("JIRA_URL", "https://acme.atlassian.net"),
                ("JIRA_USERNAME", "ada@acme.com"),
                ("TOOLSETS", "default"),
            ]),
        ),
        (
            "linear-nest",
            ServerConfig::new(
                "/usr/local/bin/node",
                [
                    "/Users/ada/agents-kitbag/src/secret-runner.js",
                    "--secret",
                    "LINEAR_MCP_TOKEN=keychain:agents-kitbag-linear-nest",
                    "--",
                    "npx",
                    "-y",
                    "mcp-remote",
                    "https://mcp.linear.app/mcp",
                    "--header",
                    "Authorization: Bearer ${LINEAR_MCP_TOKEN}",
                ],
            )
            .with_env(&[]),
        ),
        (
            "asana",
            ServerConfig::new("npx", ["-y", "@roychri/mcp-server-asana@beta"])
                .with_env(&[("ASANA_ACCESS_TOKEN", BAD_TOKEN)]),
        ),
        (
            "github",
            ServerConfig::new(
                RUNNER,
                [
                    "--secret",
                    "GITHUB_MCP_TOKEN=gh:",
                    "--",
                    "npx",
                    "-y",
                    "mcp-remote",
                    "https://api.githubcopilot.com/mcp/",
                    "--header",
                    "Authorization: Bearer ${GITHUB_MCP_TOKEN}",
                ],
            )
            .with_env(&[]),
        ),
    ];
    // One write, so the demo starts without a backup of its own making.
    let entries: serde_json::Map<String, serde_json::Value> = servers
        .into_iter()
        .map(|(key, server)| {
            (
                key.to_owned(),
                serde_json::to_value(server).expect("a server is JSON"),
            )
        })
        .collect();
    let text = serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": entries }))
        .expect("a config is JSON");
    if let Err(error) =
        std::fs::create_dir_all(folder).and_then(|()| std::fs::write(config.path(), text))
    {
        log::warn!("could not write the demo config: {error}");
    }
    Backend {
        env,
        runner: Box::new(DemoRunner::default()),
        http: Box::new(DemoHttp),
        config,
        runner_path: RUNNER.to_owned(),
        log_dir: None,
        demo: true,
    }
}
