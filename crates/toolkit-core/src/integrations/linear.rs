use super::*;
use crate::health::{bearer_token, classify};
use crate::http::Request;

const GRAPHQL: &str = "https://api.linear.app/graphql";

fn viewer(token: &str, fields: &str) -> Request {
    let body = serde_json::json!({ "query": format!("{{ viewer {{ {fields} }} }}") });
    Request::post(GRAPHQL, body.to_string())
        .header("Content-Type", "application/json")
        .header("Authorization", token)
}

pub static LINEAR: Integration = Integration {
    key: "linear",
    name: "Linear",
    instances: Instances::Multi {
        prefix: "linear-",
        noun: "instance",
    },
    fields: &[Field {
        label: "API key",
        hint: "lin_api_...",
        ..TOKEN_FIELD
    }],
    steps: &[
        "API keys belong to one workspace: switch to the right one first (top left in Linear).",
        "Go to Settings, Security & access, Personal API keys.",
        "Click \"Create key\" and paste it here.",
    ],
    token_url: Some("https://linear.app/settings/account/security"),
    token: TokenSource::FieldInHeader {
        env_var: "LINEAR_MCP_TOKEN",
    },
    launcher: Launcher::Npx,
    validate: |_, token, http| {
        let response = http.send(&viewer(token, "id name email"))?;
        let json = response.json();
        let user = &json["data"]["viewer"];
        match text(&user["name"]) {
            Some(name) => Ok(match text(&user["email"]) {
                Some(email) => format!("user: {name}, {email}"),
                None => format!("user: {name}"),
            }),
            None => Err(format!(
                "Authentication failed: {}",
                text(&json["errors"][0]["message"])
                    .unwrap_or_else(|| format!("HTTP {}", response.status))
            )),
        }
    },
    build: |_, token, packages| {
        let header = format!("Authorization: Bearer {token}");
        Ok(packages
            .entry(
                "mcp-remote",
                &["https://mcp.linear.app/mcp", "--header", &header],
            )?
            .with_env(&[]))
    },
    describe: no_rows,
    prefill: no_prefill,
    check: |server, ctx| {
        let Some(token) = ctx.secret(server, bearer_token(server)) else {
            return Health::error("token missing from args");
        };
        let response = ctx.http.send(&viewer(token.expose(), "id"));
        match &response {
            Ok(answer) if answer.ok() => match text(&answer.json()["data"]["viewer"]["id"]) {
                Some(_) => Health::ok(),
                None => Health::expired("invalid token"),
            },
            _ => classify(&response),
        }
    },
};
