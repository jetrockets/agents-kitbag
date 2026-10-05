use super::*;
use crate::health::classify;
use crate::http::Request;

const ME: &str = "https://api.notion.com/v1/users/me";

fn me(token: &str) -> Request {
    Request::get(ME)
        .bearer(token)
        .header("Notion-Version", "2022-06-28")
}

pub static NOTION: Integration = Integration {
    key: "notion",
    name: "Notion",
    instances: Instances::Multi {
        prefix: "notion-",
        noun: "workspace",
    },
    fields: &[Field {
        label: "Access token",
        hint: "ntn_...",
        ..TOKEN_FIELD
    }],
    steps: &[
        "Notion tokens belong to one workspace: add each workspace separately.",
        "On the tokens page click \"New token\" and pick the workspace the assistant should reach.",
        "Name it (for example \"MCP\"), copy the token and paste it here.",
    ],
    token_url: Some("https://www.notion.so/developers/tokens"),
    token: TokenSource::Field,
    launcher: Launcher::Npx,
    validate: |_, token, http| {
        account(http.send(&me(token)), |json| {
            text(&json["name"]).or_else(|| text(&json["bot"]["owner"]["user"]["name"]))
        })
    },
    build: |_, token, packages| {
        Ok(packages
            .entry("@notionhq/notion-mcp-server", &[])?
            .with_env(&[("NOTION_TOKEN", token)]))
    },
    describe: no_rows,
    prefill: no_prefill,
    check: |server, ctx| match ctx.secret(server, server.env_var("NOTION_TOKEN")) {
        None => Health::error("token missing"),
        Some(token) => classify(&ctx.http.send(&me(token.expose()))),
    },
};
