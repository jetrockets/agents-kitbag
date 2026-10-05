//! Does a configured token still work?

use crate::config::ServerConfig;
use crate::http::{Http, Response};
use crate::integrations;
use crate::op::{self, OnePassword};
use crate::runner;
use crate::secret::Secret;
use crate::store::{SecretRef, Stores};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// The service refused the token: expired, revoked or wrong.
    Expired,
    /// The check itself failed: no network, missing credentials.
    Error,
    /// Nothing to check, or nothing that can be checked now.
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub status: Status,
    pub detail: Option<String>,
}

impl Health {
    pub fn ok() -> Self {
        Self {
            status: Status::Ok,
            detail: None,
        }
    }

    pub fn expired(detail: impl Into<String>) -> Self {
        Self {
            status: Status::Expired,
            detail: Some(detail.into()),
        }
    }

    pub fn error(detail: impl Into<String>) -> Self {
        Self {
            status: Status::Error,
            detail: Some(detail.into()),
        }
    }

    pub fn skip(detail: impl Into<String>) -> Self {
        Self {
            status: Status::Skip,
            detail: Some(detail.into()),
        }
    }
}

/// What a service's answer says about the token.
pub fn classify(response: &Result<Response, String>) -> Health {
    match response {
        Err(error) => Health::error(error.clone()),
        Ok(response) if response.ok() => Health::ok(),
        Ok(response) if matches!(response.status, 401 | 403) => {
            Health::expired(format!("HTTP {}", response.status))
        }
        Ok(response) => Health::error(format!("HTTP {}", response.status)),
    }
}

/// The token in `--header "Authorization: Bearer <token>"`.
pub fn bearer_token(server: &ServerConfig) -> Option<&str> {
    server
        .args
        .windows(2)
        .filter(|pair| pair[0] == "--header")
        .find_map(|pair| {
            let (name, value) = pair[1].split_once(':')?;
            if !name.eq_ignore_ascii_case("authorization") {
                return None;
            }
            let value = value.trim_start();
            let (scheme, token) = value.split_once(char::is_whitespace)?;
            scheme
                .eq_ignore_ascii_case("bearer")
                .then_some(token.trim_start())
        })
}

/// What a check needs: the stores a token may be behind, and the network.
pub struct HealthCtx<'a> {
    pub stores: Stores<'a>,
    pub op: Option<OnePassword<'a>>,
    pub http: &'a dyn Http,
}

impl HealthCtx<'_> {
    /// A configured token may be the literal value, an `op://` reference, a
    /// `${VAR}` placeholder pointing at one in `env` (mcp-remote expands
    /// those in headers at launch), or a binding on the runner's command
    /// line. A check needs the real value behind all four.
    pub fn secret(&self, server: &ServerConfig, value: Option<&str>) -> Option<Secret> {
        let mut value = value.map(str::to_owned);
        let placeholder = value
            .as_deref()
            .and_then(|v| v.strip_prefix("${")?.strip_suffix('}'))
            .map(str::to_owned);
        if let Some(variable) = placeholder {
            if let Some(reference) = runner::binding(server, Some(&variable)) {
                return self.stored(&reference);
            }
            value = server.env_var(&variable).map(str::to_owned);
        }
        match value.filter(|v| !v.is_empty()) {
            None => self.stored(&runner::binding(server, None)?),
            Some(reference) if op::is_secret_ref(&reference) => self.op.as_ref()?.read(&reference),
            Some(literal) => Some(Secret::new(literal)),
        }
    }

    fn stored(&self, reference: &str) -> Option<Secret> {
        self.stores.read(&SecretRef::parse(reference).ok()?)
    }

    /// One server's health.
    pub fn check(&self, key: &str, server: &ServerConfig, op_usable: bool) -> Health {
        // Every op-backed server would otherwise report "token missing" when
        // 1Password is locked, which sends people off re-running setup for
        // nothing.
        if op::is_wrapped(server) && !op_usable {
            return Health::skip("1Password is locked. Unlock it to check.");
        }
        // A runner-backed server whose secret cannot be read will not start.
        if let Some(reference) = runner::binding(server, None)
            && self.stored(&reference).is_none()
        {
            return Health::skip(format!(
                "The credential store has no {reference}. Set it up again."
            ));
        }
        let health = match integrations::for_config_key(key) {
            Some(integration) => (integration.check)(server, self),
            None => return Health::skip("Not a toolkit integration"),
        };
        // 1Password answered the vault list but not the read: it locked in
        // between, or the prompt was dismissed. The token is not missing.
        if op::is_wrapped(server)
            && health.status == Status::Error
            && health
                .detail
                .as_deref()
                .is_some_and(|d| d.contains("missing"))
        {
            return Health::skip(
                "1Password did not hand over the token. Unlock it and check again.",
            );
        }
        health
    }

    /// Every server's health, checked side by side.
    pub fn check_all(&self, servers: &[(String, ServerConfig)]) -> Vec<(String, Health)> {
        let op_backed = servers.iter().any(|(_, server)| op::is_wrapped(server));
        let op_usable = op_backed && self.op.as_ref().is_some_and(OnePassword::usable);
        std::thread::scope(|scope| {
            let checks: Vec<_> = servers
                .iter()
                .map(|(key, server)| (key, scope.spawn(move || self.check(key, server, op_usable))))
                .collect();
            checks
                .into_iter()
                .map(|(key, check)| {
                    let health = check
                        .join()
                        .unwrap_or_else(|_| Health::error("unexpected error"));
                    (key.clone(), health)
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Env, Os};
    use crate::testing::{FakeHttp, FakeRunner, fail, ok};

    fn env() -> Env {
        Env::with(Os::Mac, "/Users/me", &[("USER", "me")])
    }

    fn ctx<'a>(env: &'a Env, runner: &'a FakeRunner, http: &'a FakeHttp) -> HealthCtx<'a> {
        HealthCtx {
            stores: Stores { env, runner },
            op: Some(OnePassword {
                binary: "/opt/homebrew/bin/op".into(),
                runner,
            }),
            http,
        }
    }

    fn asana(token: &str) -> ServerConfig {
        ServerConfig::new("npx", ["-y", "@roychri/mcp-server-asana@beta"])
            .with_env(&[("ASANA_ACCESS_TOKEN", token)])
    }

    #[test]
    fn answers_are_classified() {
        let response = |status| {
            Ok(Response {
                status,
                body: String::new(),
            })
        };
        assert_eq!(classify(&response(200)).status, Status::Ok);
        assert_eq!(classify(&response(401)), Health::expired("HTTP 401"));
        assert_eq!(classify(&response(403)).status, Status::Expired);
        assert_eq!(classify(&response(500)), Health::error("HTTP 500"));
        assert_eq!(classify(&Err("offline".into())), Health::error("offline"));
    }

    #[test]
    fn the_bearer_token_is_found_among_several_headers() {
        let server = ServerConfig::new(
            "npx",
            [
                "-y",
                "mcp-remote",
                "https://x",
                "--header",
                "X-MCP-Toolsets: a,b",
                "--header",
                "authorization:  bearer  tok",
            ],
        );
        assert_eq!(bearer_token(&server), Some("tok"));
        assert_eq!(bearer_token(&ServerConfig::new("npx", ["--header"])), None);
        assert_eq!(bearer_token(&asana("t")), None);
    }

    #[test]
    fn a_literal_token_is_itself() {
        let (env, runner, http) = (env(), FakeRunner::default(), FakeHttp::default());
        let server = asana("tok");
        let secret =
            ctx(&env, &runner, &http).secret(&server, server.env_var("ASANA_ACCESS_TOKEN"));
        assert_eq!(secret.unwrap().expose(), "tok");
    }

    #[test]
    fn an_op_reference_is_read_from_1password() {
        let runner = FakeRunner::default().on("op", "read", ok("real\n"));
        let (env, http) = (env(), FakeHttp::default());
        let server = asana("op://Private/Claude MCP - asana/credential");
        let secret =
            ctx(&env, &runner, &http).secret(&server, server.env_var("ASANA_ACCESS_TOKEN"));
        assert_eq!(secret.unwrap().expose(), "real");
    }

    #[test]
    fn a_runner_binding_is_read_from_the_store_for_both_runners() {
        let runner = FakeRunner::default().on("security", "claude-mcp-asana", ok("real\n"));
        let (env, http) = (env(), FakeHttp::default());
        for command in [
            ServerConfig::new(
                "/usr/local/bin/node",
                [
                    "/x/secret-runner.js",
                    "--secret",
                    "ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana",
                    "--",
                    "npx",
                ],
            ),
            ServerConfig::new(
                "/Applications/Claude Toolkit.app/Contents/MacOS/claude-toolkit-runner",
                [
                    "--secret",
                    "ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana",
                    "--",
                    "npx",
                ],
            ),
        ] {
            let secret =
                ctx(&env, &runner, &http).secret(&command, command.env_var("ASANA_ACCESS_TOKEN"));
            assert_eq!(secret.unwrap().expose(), "real");
        }
    }

    #[test]
    fn a_header_placeholder_follows_the_binding_or_the_env() {
        let runner = FakeRunner::default()
            .on("gh", "token", ok("gho_real\n"))
            .on("op", "read", ok("op_real\n"));
        let (env, http) = (env(), FakeHttp::default());
        let ctx = ctx(&env, &runner, &http);

        let through_runner = ServerConfig::new(
            "/x/claude-toolkit-runner",
            [
                "--secret",
                "GITHUB_MCP_TOKEN=gh:",
                "--",
                "npx",
                "--header",
                "Authorization: Bearer ${GITHUB_MCP_TOKEN}",
            ],
        );
        let secret = ctx.secret(&through_runner, bearer_token(&through_runner));
        assert_eq!(secret.unwrap().expose(), "gho_real");

        let through_op = ServerConfig::new(
            "/opt/homebrew/bin/op",
            [
                "run",
                "--no-masking",
                "--",
                "npx",
                "--header",
                "Authorization: Bearer ${LINEAR_MCP_TOKEN}",
            ],
        )
        .with_env(&[("LINEAR_MCP_TOKEN", "op://Private/x/credential")]);
        let secret = ctx.secret(&through_op, bearer_token(&through_op));
        assert_eq!(secret.unwrap().expose(), "op_real");
    }

    #[test]
    fn a_valid_token_is_ok_and_a_refused_one_expired() {
        let (env, runner) = (env(), FakeRunner::default());
        let good = FakeHttp::default().on("app.asana.com", 200, "{}");
        assert_eq!(
            ctx(&env, &runner, &good).check("asana", &asana("tok"), false),
            Health::ok()
        );
        assert_eq!(
            good.requests()[0].headers,
            [("Authorization".to_owned(), "Bearer tok".to_owned())]
        );

        let bad = FakeHttp::default().on("app.asana.com", 401, "{}");
        assert_eq!(
            ctx(&env, &runner, &bad).check("asana", &asana("tok"), false),
            Health::expired("HTTP 401")
        );
    }

    #[test]
    fn a_locked_1password_is_reported_as_locked_not_as_a_missing_token() {
        let runner = FakeRunner::default().on("op", "vault", fail(1, "locked"));
        let (env, http) = (env(), FakeHttp::default());
        let server =
            op::wrap_with_op_run(asana("op://Private/x/credential"), "/opt/homebrew/bin/op");
        let results = ctx(&env, &runner, &http).check_all(&[("asana".to_owned(), server)]);
        assert_eq!(results[0].1.status, Status::Skip);
        assert!(
            results[0]
                .1
                .detail
                .as_deref()
                .unwrap()
                .contains("1Password is locked")
        );
        assert!(http.requests().is_empty());
    }

    #[test]
    fn a_1password_that_will_not_read_is_not_a_missing_token() {
        // The vaults list, but the read is refused.
        let runner = FakeRunner::default().on("op", "vault", ok("[]")).on(
            "op",
            "read",
            fail(1, "authorization prompt dismissed"),
        );
        let (env, http) = (env(), FakeHttp::default());
        let server =
            op::wrap_with_op_run(asana("op://Private/x/credential"), "/opt/homebrew/bin/op");
        let health = ctx(&env, &runner, &http).check("asana", &server, true);
        assert_eq!(health.status, Status::Skip);
        assert!(
            health
                .detail
                .unwrap()
                .contains("1Password did not hand over the token")
        );
        assert!(http.requests().is_empty());
    }

    #[test]
    fn a_binding_with_nothing_behind_it_says_to_set_up_again() {
        let runner = FakeRunner::default().on("security", "find-generic-password", fail(44, ""));
        let (env, http) = (env(), FakeHttp::default());
        let server = ServerConfig::new(
            "/x/claude-toolkit-runner",
            ["--secret", "T=keychain:claude-mcp-asana", "--", "npx"],
        );
        let health = ctx(&env, &runner, &http).check("asana", &server, false);
        assert_eq!(health.status, Status::Skip);
        assert!(health.detail.unwrap().contains("keychain:claude-mcp-asana"));
    }

    #[test]
    fn every_server_gets_an_answer_in_order() {
        let (env, runner) = (env(), FakeRunner::default());
        let http =
            FakeHttp::default()
                .on("app.asana.com", 200, "{}")
                .on("api.figma.com", 403, "{}");
        let servers = [
            ("asana".to_owned(), asana("a")),
            (
                "ado-acme".to_owned(),
                ServerConfig::new("npx", ["-y", "@azure-devops/mcp", "acme"]),
            ),
            (
                "figma".to_owned(),
                ServerConfig::new("npx", ["-y", "figma-developer-mcp"])
                    .with_env(&[("FIGMA_API_KEY", "f")]),
            ),
            ("someone-elses".to_owned(), ServerConfig::new("x", ["y"])),
        ];
        let results = ctx(&env, &runner, &http).check_all(&servers);
        let statuses: Vec<_> = results
            .iter()
            .map(|(k, h)| (k.as_str(), h.status))
            .collect();
        assert_eq!(
            statuses,
            [
                ("asana", Status::Ok),
                ("ado-acme", Status::Skip),
                ("figma", Status::Expired),
                ("someone-elses", Status::Skip),
            ]
        );
    }
}
