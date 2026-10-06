use base64::Engine;

use super::*;
use crate::health::classify;
use crate::http::Request;
use crate::validation::jira_base;

fn myself(url: &str, email: &str, token: &str) -> Request {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("{email}:{token}"));
    Request::get(format!("{url}/rest/api/3/myself"))
        .header("Authorization", format!("Basic {basic}"))
}

pub static JIRA: Integration = Integration {
    key: "jira",
    name: "Jira",
    instances: Instances::Multi {
        prefix: "jira-",
        noun: "instance",
    },
    fields: &[
        Field {
            id: "url",
            label: "Jira URL",
            hint: "https://yourcompany.atlassian.net",
            kind: FieldKind::Text,
        },
        Field {
            id: "email",
            label: "Atlassian account email",
            hint: "you@company.com",
            kind: FieldKind::Text,
        },
        Field {
            label: "API token",
            ..TOKEN_FIELD
        },
    ],
    steps: &[
        "Create an API token for your Atlassian account and paste it here.",
        "One token works for all your Jira sites.",
    ],
    token_url: Some("https://id.atlassian.com/manage-profile/security/api-tokens"),
    token: TokenSource::Field,
    launcher: Launcher::Uvx,
    validate: |values, token, http| {
        let url = jira_base(&required(values, "url", "Jira URL")?);
        let email = required(values, "email", "Email")?;
        let response = http.send(&myself(&url, &email, token))?;
        if !response.ok() {
            return Err(format!(
                "Authentication failed (HTTP {}). Check the URL, the email and the token.",
                response.status
            ));
        }
        Ok(text(&response.json()["displayName"])
            .map(|name| format!("user: {name}"))
            .unwrap_or_else(|| "account".to_owned()))
    },
    build: |values, token, _| {
        Ok(ServerConfig::new("uvx", ["mcp-atlassian"]).with_env(&[
            ("JIRA_URL", &jira_base(value(values, "url"))),
            ("JIRA_USERNAME", value(values, "email")),
            ("JIRA_API_TOKEN", token),
            ("TOOLSETS", "default"),
        ]))
    },
    describe: |server| {
        vec![
            (
                "URL",
                server.env_var("JIRA_URL").unwrap_or("unknown").to_owned(),
            ),
            (
                "Email",
                server
                    .env_var("JIRA_USERNAME")
                    .unwrap_or("unknown")
                    .to_owned(),
            ),
        ]
    },
    prefill: |server| {
        [("url", "JIRA_URL"), ("email", "JIRA_USERNAME")]
            .into_iter()
            .filter_map(|(id, var)| Some((id.to_owned(), server.env_var(var)?.to_owned())))
            .collect()
    },
    check: |server, ctx| {
        let token = ctx.secret(server, server.env_var("JIRA_API_TOKEN"));
        match (
            server.env_var("JIRA_URL"),
            server.env_var("JIRA_USERNAME"),
            token,
        ) {
            (Some(url), Some(email), Some(token)) => {
                classify(&ctx.http.send(&myself(url, email, token.expose())))
            }
            _ => Health::error("credentials missing"),
        }
    },
};
