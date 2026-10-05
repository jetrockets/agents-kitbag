//! Setting an integration up: validate, store the token, build the server,
//! write the config. Nothing is written unless every step before it worked.

use crate::config::ConfigFile;
use crate::exec::CommandRunner;
use crate::gh::Gh;
use crate::health::HealthCtx;
use crate::http::Http;
use crate::integrations::{Integration, TOKEN, TokenSource, Values};
use crate::op::OnePassword;
use crate::packages::Packages;
use crate::platform::Env;
use crate::secret::Secret;
use crate::storage::{self, StoreChoice};
use crate::store::Stores;

/// Everything the toolkit reaches outside itself through.
pub struct Ctx<'a> {
    pub env: &'a Env,
    pub runner: &'a dyn CommandRunner,
    pub http: &'a dyn Http,
    pub packages: &'a dyn Packages,
    pub config: &'a ConfigFile,
    /// Where `claude-toolkit-runner` is, as written into the config.
    pub runner_path: &'a str,
}

impl<'a> Ctx<'a> {
    pub fn stores(&self) -> Stores<'a> {
        Stores {
            env: self.env,
            runner: self.runner,
        }
    }

    pub fn gh(&self) -> Gh<'a> {
        Gh {
            env: self.env,
            runner: self.runner,
        }
    }

    pub fn health(&self) -> HealthCtx<'a> {
        HealthCtx {
            stores: self.stores(),
            op: OnePassword::find(self.env, self.runner),
            http: self.http,
        }
    }
}

pub struct Request {
    pub integration: &'static Integration,
    /// The instance's name, for a multi-instance integration.
    pub instance: Option<String>,
    pub values: Values,
    pub store: StoreChoice,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub key: String,
    /// Who the service says the token belongs to.
    pub account: String,
    /// Something that works but could be better.
    pub warning: Option<String>,
}

pub fn run(ctx: &Ctx, request: &Request) -> Result<Done, String> {
    let integration = request.integration;
    let key = integration.config_key(request.instance.as_deref())?;

    let token = match integration.token {
        TokenSource::None => None,
        TokenSource::Field | TokenSource::FieldInHeader { .. } => {
            match request
                .values
                .get(TOKEN)
                .map(|t| t.trim())
                .unwrap_or_default()
            {
                "" => return Err("Token is required".to_owned()),
                token => Some(Secret::new(token)),
            }
        }
        TokenSource::GhCli { .. } => Some(
            ctx.gh()
                .token()
                .ok_or("The GitHub CLI has no token. Sign in with it first.")?,
        ),
    };
    let exposed = token.as_ref().map(Secret::expose).unwrap_or_default();

    let account = (integration.validate)(&request.values, exposed, ctx.http)?;

    let server = match &token {
        None => (integration.build)(&request.values, "", ctx.packages)?,
        Some(token) => {
            let stored = storage::keep(
                ctx.env,
                ctx.runner,
                ctx.runner_path,
                &request.store,
                &key,
                token,
                integration.header_env_var(),
            )?;
            stored.apply((integration.build)(
                &request.values,
                &stored.value,
                ctx.packages,
            )?)
        }
    };
    ctx.config.set_server(&key, &server)?;

    let warning = (matches!(integration.token, TokenSource::GhCli { .. })
        && !ctx.gh().has_project_scope())
    .then(|| {
        "The GitHub CLI's token has no 'project' scope, so the Projects tools stay hidden."
            .to_owned()
    });
    Ok(Done {
        key,
        account,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;
    use crate::integrations::by_key;
    use crate::platform::Os;
    use crate::testing::{FakeHttp, FakeRunner, fail, ok};

    struct Npx;
    impl Packages for Npx {
        fn entry(&self, spec: &str, extra: &[&str]) -> Result<ServerConfig, String> {
            let mut server = ServerConfig::new("npx", ["-y", spec]);
            server.args.extend(extra.iter().map(|a| (*a).to_owned()));
            Ok(server)
        }
    }

    const RUNNER: &str = "/Applications/Claude Toolkit.app/Contents/MacOS/claude-toolkit-runner";

    struct World {
        _dir: tempfile::TempDir,
        env: Env,
        config: ConfigFile,
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        World {
            env: Env::with(Os::Mac, dir.path(), &[("USER", "me")]),
            config: ConfigFile::new(dir.path().join("config.json"), Os::Mac),
            _dir: dir,
        }
    }

    fn ctx<'a>(world: &'a World, runner: &'a FakeRunner, http: &'a FakeHttp) -> Ctx<'a> {
        Ctx {
            env: &world.env,
            runner,
            http,
            packages: &Npx,
            config: &world.config,
            runner_path: RUNNER,
        }
    }

    fn request(
        key: &str,
        instance: Option<&str>,
        values: &[(&str, &str)],
        store: StoreChoice,
    ) -> Request {
        Request {
            integration: by_key(key).unwrap(),
            instance: instance.map(str::to_owned),
            values: values
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            store,
        }
    }

    fn written(world: &World, key: &str) -> serde_json::Value {
        serde_json::to_value(world.config.server(key).unwrap().unwrap()).unwrap()
    }

    #[test]
    fn asana_in_plain_text_is_what_the_js_toolkit_wrote() {
        let world = world();
        let (runner, http) = (
            FakeRunner::default(),
            FakeHttp::default().on("asana.com", 200, r#"{"data":{"name":"Ada"}}"#),
        );
        let done = run(
            &ctx(&world, &runner, &http),
            &request("asana", None, &[("token", " tok ")], StoreChoice::Plain),
        )
        .unwrap();
        assert_eq!(
            done,
            Done {
                key: "asana".into(),
                account: "user: Ada".into(),
                warning: None
            }
        );
        assert_eq!(
            written(&world, "asana"),
            serde_json::json!({
                "command": "npx",
                "args": ["-y", "@roychri/mcp-server-asana@beta"],
                "env": { "ASANA_ACCESS_TOKEN": "tok" }
            })
        );
    }

    #[test]
    fn jira_in_the_keychain_keeps_the_token_out_of_the_config() {
        let world = world();
        let runner = FakeRunner::default().on("security", "add-generic-password", ok(""));
        let http = FakeHttp::default().on("/rest/api/3/myself", 200, r#"{"displayName":"Ada L"}"#);
        let done = run(
            &ctx(&world, &runner, &http),
            &request(
                "jira",
                Some("acme"),
                &[
                    ("url", "https://acme.atlassian.net/jira/x"),
                    ("email", "ada@acme.com"),
                    ("token", "tok"),
                ],
                StoreChoice::System,
            ),
        )
        .unwrap();
        assert_eq!(done.key, "jira-acme");
        assert_eq!(done.account, "user: Ada L");
        assert_eq!(
            http.requests()[0].url,
            "https://acme.atlassian.net/rest/api/3/myself"
        );
        // base64("ada@acme.com:tok")
        assert_eq!(
            http.requests()[0].headers[0].1,
            "Basic YWRhQGFjbWUuY29tOnRvaw=="
        );
        assert_eq!(
            written(&world, "jira-acme"),
            serde_json::json!({
                "command": RUNNER,
                "args": ["--secret", "JIRA_API_TOKEN=keychain:claude-mcp-jira-acme", "--", "uvx", "mcp-atlassian"],
                "env": {
                    "JIRA_URL": "https://acme.atlassian.net",
                    "JIRA_USERNAME": "ada@acme.com",
                    "TOOLSETS": "default"
                }
            })
        );
        assert!(
            !std::fs::read_to_string(world.config.path())
                .unwrap()
                .contains("tok\"")
        );
    }

    #[test]
    fn linear_in_1password_carries_a_placeholder_in_its_header() {
        let mut world = world();
        world.env = Env::with(
            Os::Mac,
            "/h",
            &[(crate::op::PATH_OVERRIDE, "/opt/homebrew/bin/op")],
        );
        let runner = FakeRunner::default().on(
            "op",
            "create",
            ok(r#"{"title":"Claude MCP - linear-nest"}"#),
        );
        let http = FakeHttp::default().on(
            "linear.app/graphql",
            200,
            r#"{"data":{"viewer":{"id":"1","name":"Ada","email":"a@b.c"}}}"#,
        );
        let store = StoreChoice::OnePassword {
            vault: "Private".into(),
        };
        let done = run(
            &ctx(&world, &runner, &http),
            &request("linear", Some("nest"), &[("token", "lin_api_x")], store),
        )
        .unwrap();
        assert_eq!(done.account, "user: Ada, a@b.c");
        assert_eq!(
            http.requests()[0].headers[1],
            ("Authorization".to_owned(), "lin_api_x".to_owned())
        );
        assert_eq!(
            written(&world, "linear-nest"),
            serde_json::json!({
                "command": "/opt/homebrew/bin/op",
                "args": [
                    "run", "--no-masking", "--",
                    "npx", "-y", "mcp-remote", "https://mcp.linear.app/mcp",
                    "--header", "Authorization: Bearer ${LINEAR_MCP_TOKEN}"
                ],
                "env": { "LINEAR_MCP_TOKEN": "op://Private/Claude MCP - linear-nest/credential" }
            })
        );
        assert!(
            !std::fs::read_to_string(world.config.path())
                .unwrap()
                .contains("lin_api_x")
        );
    }

    #[test]
    fn azure_devops_needs_no_token_and_has_no_env() {
        let world = world();
        let (runner, http) = (FakeRunner::default(), FakeHttp::default());
        let done = run(
            &ctx(&world, &runner, &http),
            &request(
                "azure-devops",
                Some("contoso"),
                &[("org", "https://dev.azure.com/Contoso/Proj")],
                StoreChoice::Plain,
            ),
        )
        .unwrap();
        assert_eq!(done.account, "organization: Contoso");
        assert_eq!(
            written(&world, "ado-contoso"),
            serde_json::json!({ "command": "npx", "args": ["-y", "@azure-devops/mcp", "Contoso"] })
        );
        assert!(http.requests().is_empty());
    }

    #[test]
    fn github_asks_gh_at_launch_and_warns_about_the_project_scope() {
        let world = world();
        let runner = FakeRunner::default().on("gh", "token", ok("gho_abc\n")).on(
            "gh",
            "status",
            ok("Token scopes: 'repo', 'read:org'"),
        );
        let http = FakeHttp::default().on("api.github.com/user", 200, r#"{"login":"ada"}"#);
        let done = run(
            &ctx(&world, &runner, &http),
            &request("github", None, &[], StoreChoice::Gh),
        )
        .unwrap();
        assert_eq!(done.account, "user: ada");
        assert!(done.warning.unwrap().contains("project"));
        assert_eq!(
            written(&world, "github"),
            serde_json::json!({
                "command": RUNNER,
                "args": [
                    "--secret", "GITHUB_MCP_TOKEN=gh:", "--",
                    "npx", "-y", "mcp-remote", "https://api.githubcopilot.com/mcp/",
                    "--header", "Authorization: Bearer ${GITHUB_MCP_TOKEN}",
                    "--header", "X-MCP-Toolsets: context,repos,issues,pull_requests,users,projects",
                    "--header", "X-MCP-Features: mcp_apps_disable_form_deferral"
                ],
                "env": {}
            })
        );
    }

    #[test]
    fn github_without_a_signed_in_cli_is_refused() {
        let world = world();
        let (runner, http) = (FakeRunner::default(), FakeHttp::default());
        let error = run(
            &ctx(&world, &runner, &http),
            &request("github", None, &[], StoreChoice::Gh),
        )
        .unwrap_err();
        assert!(error.contains("GitHub CLI"), "{error}");
    }

    #[test]
    fn a_refused_token_writes_nothing() {
        let world = world();
        let (runner, http) = (
            FakeRunner::default(),
            FakeHttp::default().on("figma.com", 403, "{}"),
        );
        let error = run(
            &ctx(&world, &runner, &http),
            &request("figma", None, &[("token", "bad")], StoreChoice::Plain),
        )
        .unwrap_err();
        assert_eq!(error, "Token invalid (HTTP 403). Check it and try again.");
        assert!(!world.config.path().exists());
        assert!(
            runner.calls().is_empty(),
            "nothing is stored for a bad token"
        );
    }

    #[test]
    fn a_store_that_refuses_writes_nothing() {
        let world = world();
        let runner =
            FakeRunner::default().on("security", "add-generic-password", fail(1, "denied"));
        let http = FakeHttp::default().on("notion.com", 200, r#"{"name":"Claude MCP"}"#);
        let error = run(
            &ctx(&world, &runner, &http),
            &request(
                "notion",
                Some("acme"),
                &[("token", "ntn_x")],
                StoreChoice::System,
            ),
        )
        .unwrap_err();
        assert_eq!(error, "Could not save to macOS Keychain: denied");
        assert!(!world.config.path().exists());
    }

    #[test]
    fn missing_fields_and_bad_names_are_caught_before_any_request() {
        let world = world();
        let (runner, http) = (FakeRunner::default(), FakeHttp::default());
        let ctx = ctx(&world, &runner, &http);
        assert_eq!(
            run(&ctx, &request("asana", None, &[], StoreChoice::Plain)).unwrap_err(),
            "Token is required"
        );
        assert!(
            run(
                &ctx,
                &request(
                    "jira",
                    Some("Bad Name"),
                    &[("token", "t")],
                    StoreChoice::Plain
                )
            )
            .is_err()
        );
        assert_eq!(
            run(
                &ctx,
                &request("jira", Some("acme"), &[("token", "t")], StoreChoice::Plain)
            )
            .unwrap_err(),
            "Jira URL is required"
        );
        assert!(http.requests().is_empty());
    }

    #[test]
    fn no_network_is_said_plainly() {
        let world = world();
        let (runner, http) = (FakeRunner::default(), FakeHttp::default().down("asana.com"));
        let error = run(
            &ctx(&world, &runner, &http),
            &request("asana", None, &[("token", "t")], StoreChoice::Plain),
        )
        .unwrap_err();
        assert_eq!(error, "network error");
    }
}
