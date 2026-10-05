use super::*;
use crate::health::classify;
use crate::http::Request;

const ME: &str = "https://api.figma.com/v1/me";

pub static FIGMA: Integration = Integration {
    key: "figma",
    name: "Figma",
    instances: Instances::Single {
        config_key: "figma",
    },
    fields: &[Field {
        label: "Personal access token",
        hint: "figd_...",
        ..TOKEN_FIELD
    }],
    steps: &[
        "In Figma open Settings (your avatar, top left).",
        "On the Security tab, find \"Personal access tokens\" and generate a new one.",
        "Scopes: File content, File metadata, Comments and Dev resources, all Read.",
        "Copy the token (it starts with figd_) and paste it here.",
    ],
    token_url: Some("https://www.figma.com/files"),
    token: TokenSource::Field,
    launcher: Launcher::Npx,
    validate: |_, token, http| {
        account(
            http.send(&Request::get(ME).header("X-Figma-Token", token)),
            |json| text(&json["handle"]).or_else(|| text(&json["email"])),
        )
    },
    build: |_, token, packages| {
        Ok(packages
            .entry("figma-developer-mcp", &["--stdio"])?
            .with_env(&[("FIGMA_API_KEY", token)]))
    },
    describe: no_rows,
    prefill: no_prefill,
    check: |server, ctx| match ctx.secret(server, server.env_var("FIGMA_API_KEY")) {
        None => Health::error("token missing"),
        Some(token) => classify(
            &ctx.http
                .send(&Request::get(ME).header("X-Figma-Token", token.expose())),
        ),
    },
};
