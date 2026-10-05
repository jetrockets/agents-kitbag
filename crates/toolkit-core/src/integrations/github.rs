use super::*;
use crate::health::{bearer_token, classify};
use crate::http::Request;

const USER: &str = "https://api.github.com/user";

/// `projects` is not in the server's default toolsets, so without the
/// X-MCP-Toolsets header the Projects tools never show up. It is enabled
/// beside the everyday repo, issue and pull request toolsets.
const TOOLSETS: &str = "context,repos,issues,pull_requests,users,projects";

/// Since October 2026 the hosted server answers `issue_write` and the pull
/// request write tools with a form to confirm instead of writing. Claude
/// Desktop does not draw that form (github/github-mcp-server#2823), so
/// nothing is ever written. This flag makes the tools write directly again.
const FEATURES: &str = "mcp_apps_disable_form_deferral";

pub static GITHUB: Integration = Integration {
    key: "github",
    name: "GitHub",
    instances: Instances::Single {
        config_key: "github",
    },
    fields: &[],
    steps: &[
        "GitHub uses the token of the GitHub CLI (gh). Nothing to paste.",
        "The CLI must be installed and signed in.",
    ],
    token_url: None,
    token: TokenSource::GhCli {
        env_var: "GITHUB_MCP_TOKEN",
    },
    launcher: Launcher::Npx,
    validate: |_, token, http| {
        account(http.send(&Request::get(USER).bearer(token)), |json| {
            text(&json["login"]).map(|login| format!("user: {login}"))
        })
    },
    // GitHub's official server (github/github-mcp-server), reached at its
    // hosted endpoint through mcp-remote, as Linear is.
    build: |_, token, packages| {
        let authorization = format!("Authorization: Bearer {token}");
        let toolsets = format!("X-MCP-Toolsets: {TOOLSETS}");
        let features = format!("X-MCP-Features: {FEATURES}");
        Ok(packages
            .entry(
                "mcp-remote",
                &[
                    "https://api.githubcopilot.com/mcp/",
                    "--header",
                    &authorization,
                    "--header",
                    &toolsets,
                    "--header",
                    &features,
                ],
            )?
            .with_env(&[]))
    },
    describe: no_rows,
    prefill: no_prefill,
    check: |server, ctx| match ctx.secret(server, bearer_token(server)) {
        None => Health::error("token missing from args"),
        Some(token) => classify(&ctx.http.send(&Request::get(USER).bearer(token.expose()))),
    },
};
