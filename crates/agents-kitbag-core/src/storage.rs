//! "Where should this token be kept?"
//!
//! Keeping a token out of `claude_desktop_config.json` takes two steps: put
//! it in a store, and rewrite the finished server config so a wrapper fetches
//! it at launch. [`keep`] does the first and returns the value the
//! integration builds its config with; [`Stored::apply`] does the second.

use serde_json::Value;

use crate::config::ServerConfig;
use crate::exec::CommandRunner;
use crate::op::{self, OnePassword};
use crate::platform::{Env, Os};
use crate::runner;
use crate::secret::Secret;
use crate::store::{Backend, SecretRef, Stores};

/// Marks where a token would have gone in an integration's `env`, so `apply`
/// can find the variable name and move it into a `--secret` binding.
pub const PLACEHOLDER: &str = "__AGENTS_KITBAG_SECRET__";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreChoice {
    /// The GitHub CLI already holds the token in the OS keychain, so the best
    /// option is not to keep a second copy: the wrapper asks gh at launch.
    Gh,
    OnePassword {
        vault: String,
    },
    /// The Keychain on macOS, DPAPI on Windows.
    System,
    /// In the config file, in plain text.
    Plain,
}

/// The stores that work on this machine, best first. `Plain` always does.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Available {
    pub gh: bool,
    /// The 1Password vaults, when op can reach them.
    pub vaults: Option<Vec<String>>,
    /// op is installed but cannot reach the vaults: the desktop app's CLI
    /// integration is off, or 1Password is locked.
    pub op_unreachable: bool,
    pub system: Option<Backend>,
}

pub fn available(env: &Env, runner: &dyn CommandRunner) -> Available {
    let stores = Stores { env, runner };
    let op = OnePassword::find(env, runner);
    let vaults = op.as_ref().and_then(OnePassword::vaults);
    let system = Backend::system(env.os);
    Available {
        gh: stores.usable(Backend::Gh),
        op_unreachable: op.is_some() && vaults.is_none(),
        vaults,
        system: stores.usable(system).then_some(system),
    }
}

impl Available {
    /// The vault to offer first.
    pub fn default_vault(&self) -> Option<&str> {
        let vaults = self.vaults.as_deref()?;
        vaults
            .iter()
            .find(|v| *v == "Private")
            .or(vaults.first())
            .map(String::as_str)
    }

    /// The best store for a new token: the system's, else the config file.
    /// For GitHub, the CLI itself when it is signed in.
    pub fn default_choice(&self, gh_source: bool) -> StoreChoice {
        if gh_source && self.gh {
            StoreChoice::Gh
        } else if self.system.is_some() {
            StoreChoice::System
        } else {
            StoreChoice::Plain
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Rewrite {
    None,
    Op {
        binary: String,
        env_var: Option<String>,
        reference: String,
    },
    Runner {
        runner_path: String,
        env_var: Option<String>,
        reference: String,
    },
}

/// A token that has been put where it was asked to go.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    /// What the integration writes where the token would go.
    pub value: String,
    rewrite: Rewrite,
}

/// Puts the token in the chosen store. A store that refuses is an error with
/// its own message; nothing falls back to plain text on its own.
///
/// `env_var` is for integrations that carry the token in `args` rather than
/// `env` (GitHub and Linear pass it as an Authorization header): the
/// placeholder `${VAR}` goes into the header, which `mcp-remote` expands from
/// the environment the wrapper supplies.
pub fn keep(
    env: &Env,
    runner: &dyn CommandRunner,
    runner_path: &str,
    choice: &StoreChoice,
    config_key: &str,
    token: &Secret,
    env_var: Option<&str>,
) -> Result<Stored, String> {
    let env_var = env_var.map(str::to_owned);
    let through_runner = |reference: SecretRef| Stored {
        value: match &env_var {
            Some(var) => format!("${{{var}}}"),
            None => PLACEHOLDER.to_owned(),
        },
        rewrite: Rewrite::Runner {
            runner_path: runner_path.to_owned(),
            env_var: env_var.clone(),
            reference: reference.to_string(),
        },
    };
    match choice {
        StoreChoice::Plain => Ok(Stored {
            value: token.expose().to_owned(),
            rewrite: Rewrite::None,
        }),
        StoreChoice::Gh => Ok(through_runner(SecretRef::for_key(Backend::Gh, config_key))),
        StoreChoice::System => {
            let backend = Backend::system(env.os);
            let reference = SecretRef::for_key(backend, config_key);
            Stores { env, runner }
                .store(&reference, token)
                .map_err(|error| format!("Could not save to {}: {error}", backend.label()))?;
            Ok(through_runner(reference))
        }
        StoreChoice::OnePassword { vault } => {
            let op = OnePassword::find(env, runner).ok_or("1Password CLI (op) is not installed")?;
            let reference = op
                .create_item(&op::item_title(config_key), vault, token)
                .map_err(|error| format!("Could not save to 1Password: {error}"))?;
            Ok(Stored {
                value: match &env_var {
                    Some(var) => format!("${{{var}}}"),
                    None => reference.clone(),
                },
                rewrite: Rewrite::Op {
                    binary: op.binary,
                    env_var,
                    reference,
                },
            })
        }
    }
}

impl Stored {
    /// Rewrites the finished server config to fetch the secret at launch.
    /// A server built with [`PLACEHOLDER`] for a token gets [`Self::value`]
    /// there first, so it can be built before the token is stored.
    pub fn apply(&self, mut server: ServerConfig) -> ServerConfig {
        if self.value != PLACEHOLDER {
            for arg in &mut server.args {
                if arg.contains(PLACEHOLDER) {
                    *arg = arg.replace(PLACEHOLDER, &self.value);
                }
            }
            for value in server.env.iter_mut().flat_map(|env| env.values_mut()) {
                if value.as_str() == Some(PLACEHOLDER) {
                    *value = Value::String(self.value.clone());
                }
            }
        }
        match &self.rewrite {
            Rewrite::None => server,
            Rewrite::Op {
                binary,
                env_var,
                reference,
            } => {
                if let Some(var) = env_var {
                    server
                        .env
                        .get_or_insert_default()
                        .insert(var.clone(), Value::String(reference.clone()));
                }
                op::wrap_with_op_run(server, binary)
            }
            Rewrite::Runner {
                runner_path,
                env_var,
                reference,
            } => {
                let env = server.env.clone().unwrap_or_default();
                // In env mode the integration put the placeholder under its
                // own variable name; the wrapper supplies that variable, so
                // it leaves `env`.
                let variables: Vec<String> = match env_var {
                    Some(var) => vec![var.clone()],
                    None => env
                        .iter()
                        .filter(|(_, v)| v.as_str() == Some(PLACEHOLDER))
                        .map(|(k, _)| k.clone())
                        .collect(),
                };
                let remaining = env
                    .into_iter()
                    .filter(|(_, v)| v.as_str() != Some(PLACEHOLDER))
                    .collect();
                let mut args: Vec<String> = variables
                    .iter()
                    .flat_map(|var| ["--secret".to_owned(), format!("{var}={reference}")])
                    .collect();
                args.push("--".to_owned());
                args.push(server.command.clone());
                args.extend(server.args.iter().cloned());
                ServerConfig {
                    command: runner_path.clone(),
                    args,
                    env: Some(remaining),
                    rest: server.rest,
                }
            }
        }
    }
}

/// Where a configured server's token is kept, for the detail pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    OnePassword,
    Store(Backend),
    /// A reference to another system's store, in a config carried over.
    Unreadable(Backend),
    ConfigFile,
    /// The service's own hosted server: it signs in by itself.
    Hosted,
}

impl Location {
    pub fn label(&self) -> String {
        match self {
            Location::OnePassword => "1Password".to_owned(),
            Location::Store(backend) => backend.label().to_owned(),
            Location::Unreadable(backend) => {
                format!("{}, not readable on this system", backend.label())
            }
            Location::ConfigFile => "Config file (plain text)".to_owned(),
            Location::Hosted => "Nowhere here: the server signs in by itself".to_owned(),
        }
    }
}

pub fn location(server: &ServerConfig, os: Os) -> Location {
    if server.url().is_some() {
        return Location::Hosted;
    }
    if op::is_wrapped(server) {
        return Location::OnePassword;
    }
    match runner::binding(server, None).map(|r| SecretRef::parse(&r)) {
        Some(Ok(reference)) if !reference.backend.readable_on(os) => {
            Location::Unreadable(reference.backend)
        }
        Some(Ok(reference)) => Location::Store(reference.backend),
        _ => Location::ConfigFile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, fail, ok};

    const RUNNER: &str = "/Applications/Agents Kitbag.app/Contents/MacOS/agents-kitbag-runner";

    fn mac() -> Env {
        Env::with(Os::Mac, "/Users/me", &[("USER", "me")])
    }

    fn asana(token: &str) -> ServerConfig {
        ServerConfig::new("npx", ["-y", "@roychri/mcp-server-asana@beta"])
            .with_env(&[("ASANA_ACCESS_TOKEN", token)])
    }

    fn linear(token: &str) -> ServerConfig {
        ServerConfig::new(
            "npx",
            [
                "-y".to_owned(),
                "mcp-remote".to_owned(),
                "https://mcp.linear.app/mcp".to_owned(),
                "--header".to_owned(),
                format!("Authorization: Bearer {token}"),
            ],
        )
        .with_env(&[])
    }

    #[test]
    fn plain_text_leaves_the_config_as_the_integration_built_it() {
        let runner = FakeRunner::default();
        let stored = keep(
            &mac(),
            &runner,
            RUNNER,
            &StoreChoice::Plain,
            "asana",
            &Secret::new("tok"),
            None,
        )
        .unwrap();
        assert_eq!(stored.value, "tok");
        assert_eq!(stored.apply(asana(&stored.value)), asana("tok"));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn the_system_store_moves_the_token_into_a_runner_binding() {
        let runner = FakeRunner::default().on("security", "add-generic-password", ok(""));
        let stored = keep(
            &mac(),
            &runner,
            RUNNER,
            &StoreChoice::System,
            "asana",
            &Secret::new("tok"),
            None,
        )
        .unwrap();
        let server = stored.apply(asana(&stored.value));
        assert_eq!(server.command, RUNNER);
        assert_eq!(
            server.args,
            [
                "--secret",
                "ASANA_ACCESS_TOKEN=keychain:agents-kitbag-asana",
                "--",
                "npx",
                "-y",
                "@roychri/mcp-server-asana@beta",
            ]
        );
        assert_eq!(server.env, Some(serde_json::Map::new()));
        assert_eq!(
            location(&server, Os::Mac),
            Location::Store(Backend::Keychain)
        );
        let json = serde_json::to_string(&server).unwrap();
        assert!(!json.contains("tok\""), "{json}");
    }

    #[test]
    fn other_variables_stay_in_env() {
        let runner = FakeRunner::default().on("security", "add-generic-password", ok(""));
        let stored = keep(
            &mac(),
            &runner,
            RUNNER,
            &StoreChoice::System,
            "jira-acme",
            &Secret::new("tok"),
            None,
        )
        .unwrap();
        let jira = ServerConfig::new("uvx", ["mcp-atlassian"]).with_env(&[
            ("JIRA_URL", "https://acme.atlassian.net"),
            ("JIRA_API_TOKEN", &stored.value),
            ("TOOLSETS", "default"),
        ]);
        let server = stored.apply(jira);
        assert_eq!(
            server.args[1],
            "JIRA_API_TOKEN=keychain:agents-kitbag-jira-acme"
        );
        assert_eq!(
            server.env_var("JIRA_URL"),
            Some("https://acme.atlassian.net")
        );
        assert_eq!(server.env_var("TOOLSETS"), Some("default"));
        assert_eq!(server.env_var("JIRA_API_TOKEN"), None);
    }

    #[test]
    fn a_header_token_becomes_a_placeholder_the_wrapper_fills() {
        let runner = FakeRunner::default().on("security", "add-generic-password", ok(""));
        let stored = keep(
            &mac(),
            &runner,
            RUNNER,
            &StoreChoice::System,
            "linear-nest",
            &Secret::new("lin_api"),
            Some("LINEAR_MCP_TOKEN"),
        )
        .unwrap();
        assert_eq!(stored.value, "${LINEAR_MCP_TOKEN}");
        let server = stored.apply(linear(&stored.value));
        assert_eq!(
            server.args[..3],
            [
                "--secret",
                "LINEAR_MCP_TOKEN=keychain:agents-kitbag-linear-nest",
                "--"
            ]
        );
        assert_eq!(
            server.args.last().unwrap(),
            "Authorization: Bearer ${LINEAR_MCP_TOKEN}"
        );
    }

    #[test]
    fn gh_keeps_no_second_copy() {
        let runner = FakeRunner::default();
        let stored = keep(
            &mac(),
            &runner,
            RUNNER,
            &StoreChoice::Gh,
            "github",
            &Secret::new("gho_x"),
            Some("GITHUB_MCP_TOKEN"),
        )
        .unwrap();
        let server = stored.apply(linear(&stored.value));
        assert_eq!(server.args[1], "GITHUB_MCP_TOKEN=gh:");
        assert!(runner.calls().is_empty());
        assert_eq!(location(&server, Os::Mac), Location::Store(Backend::Gh));
    }

    #[test]
    fn a_store_that_refuses_is_an_error_not_plain_text() {
        let runner =
            FakeRunner::default().on("security", "add-generic-password", fail(1, "User canceled"));
        let error = keep(
            &mac(),
            &runner,
            RUNNER,
            &StoreChoice::System,
            "asana",
            &Secret::new("tok"),
            None,
        )
        .unwrap_err();
        assert_eq!(error, "Could not save to macOS Keychain: User canceled");
    }

    #[test]
    fn the_default_store_is_the_systems_or_gh_for_github() {
        let all = Available {
            gh: true,
            vaults: None,
            op_unreachable: false,
            system: Some(Backend::Keychain),
        };
        assert_eq!(all.default_choice(true), StoreChoice::Gh);
        assert_eq!(all.default_choice(false), StoreChoice::System);
        assert_eq!(
            Available::default().default_choice(true),
            StoreChoice::Plain
        );
    }

    #[test]
    fn private_is_the_vault_offered_first() {
        let mut available = Available {
            vaults: Some(vec!["Work".into(), "Private".into()]),
            ..Default::default()
        };
        assert_eq!(available.default_vault(), Some("Private"));
        available.vaults = Some(vec!["Work".into(), "Team".into()]);
        assert_eq!(available.default_vault(), Some("Work"));
    }

    #[test]
    fn locations_are_read_back_from_a_config() {
        assert_eq!(location(&asana("tok"), Os::Mac), Location::ConfigFile);
        let mut hosted = ServerConfig::default();
        hosted
            .rest
            .insert("url".to_owned(), "https://mcp.figma.com/mcp".into());
        assert_eq!(location(&hosted, Os::Mac), Location::Hosted);
        let wrapped =
            op::wrap_with_op_run(asana("op://Private/x/credential"), "/opt/homebrew/bin/op");
        assert_eq!(location(&wrapped, Os::Mac), Location::OnePassword);
        let linux = ServerConfig::new(
            "node",
            [
                "/x/secret-runner.js",
                "--secret",
                "T=libsecret:agents-kitbag-a",
                "--",
                "npx",
            ],
        );
        assert_eq!(
            location(&linux, Os::Linux),
            Location::Store(Backend::Libsecret)
        );
        assert_eq!(
            location(&linux, Os::Mac),
            Location::Unreadable(Backend::Libsecret)
        );
        assert_eq!(
            location(&linux, Os::Mac).label(),
            "Secret Service (libsecret), not readable on this system"
        );
    }
}
