use super::*;
use crate::health::classify;
use crate::http::Request;

const ME: &str = "https://app.asana.com/api/1.0/users/me";

pub static ASANA: Integration = Integration {
    key: "asana",
    name: "Asana",
    instances: Instances::Single {
        config_key: "asana",
    },
    fields: &[Field {
        label: "Personal access token",
        ..TOKEN_FIELD
    }],
    steps: &[
        "One token gives access to all your Asana workspaces.",
        "Open Asana, My Settings, Apps, Personal access tokens.",
        "Click \"Create new token\" and paste it here.",
    ],
    token_url: Some("https://app.asana.com/0/my-apps"),
    token: TokenSource::Field,
    launcher: Launcher::Npx,
    validate: |_, token, http| {
        account(http.send(&Request::get(ME).bearer(token)), |json| {
            text(&json["data"]["name"]).map(|name| format!("user: {name}"))
        })
    },
    build: |_, token, packages| {
        Ok(packages
            .entry("@roychri/mcp-server-asana@beta", &[])?
            .with_env(&[("ASANA_ACCESS_TOKEN", token)]))
    },
    describe: no_rows,
    prefill: no_prefill,
    check: |server, ctx| match ctx.secret(server, server.env_var("ASANA_ACCESS_TOKEN")) {
        None => Health::error("token missing"),
        Some(token) => classify(&ctx.http.send(&Request::get(ME).bearer(token.expose()))),
    },
};
