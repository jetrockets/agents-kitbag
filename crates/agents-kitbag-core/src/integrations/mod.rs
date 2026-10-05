//! What each service needs, as data: the fields of its form, how its token
//! is validated, the server config it becomes, and how that token is checked
//! later. A new integration is one file here and one line in [`ALL`].

use std::collections::BTreeMap;

use crate::config::ServerConfig;
use crate::health::{Health, HealthCtx};
use crate::http::Http;
use crate::packages::Packages;
use crate::validation::validate_name;

mod asana;
mod azure_devops;
mod figma;
mod github;
mod jira;
mod linear;
mod notion;

/// What the person typed, by field id.
pub type Values = BTreeMap<String, String>;

/// The id of the field that holds the token.
pub const TOKEN: &str = "token";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instances {
    /// One server, under this config key.
    Single { config_key: &'static str },
    /// Any number, under `<prefix><name>`. `noun` is what one is called.
    Multi {
        prefix: &'static str,
        noun: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Secret,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub kind: FieldKind,
}

/// Where the token comes from and how it reaches the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    /// Typed into the form; reaches the server through `env`.
    Field,
    /// Typed into the form; sent as an Authorization header that names this
    /// variable when the token is kept in a store.
    FieldInHeader { env_var: &'static str },
    /// Taken from the GitHub CLI; sent as a header.
    GhCli { env_var: &'static str },
    /// The server signs in by itself (a browser login on first use).
    None,
}

/// What must be installed for the server to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launcher {
    Npx,
    Uvx,
}

pub struct Integration {
    pub key: &'static str,
    pub name: &'static str,
    pub instances: Instances,
    pub fields: &'static [Field],
    /// How to get a token, one step per line.
    pub steps: &'static [&'static str],
    /// The page those steps happen on.
    pub token_url: Option<&'static str>,
    pub token: TokenSource,
    pub launcher: Launcher,
    /// Asks the service who the token belongs to. `Ok` is that account's
    /// name, `Err` what to show in the form.
    pub validate: fn(&Values, &str, &dyn Http) -> Result<String, String>,
    /// The server config, with `token` where the token goes.
    pub build: fn(&Values, &str, &dyn Packages) -> Result<ServerConfig, String>,
    /// The rows shown for a configured server (never the token).
    pub describe: fn(&ServerConfig) -> Vec<(&'static str, String)>,
    /// The form's values when editing a configured server (never the token).
    pub prefill: fn(&ServerConfig) -> Values,
    pub check: fn(&ServerConfig, &HealthCtx) -> Health,
}

/// Every integration, in the order the sidebar lists them.
pub static ALL: [&Integration; 7] = [
    &jira::JIRA,
    &linear::LINEAR,
    &notion::NOTION,
    &azure_devops::AZURE_DEVOPS,
    &asana::ASANA,
    &github::GITHUB,
    &figma::FIGMA,
];

pub fn by_key(key: &str) -> Option<&'static Integration> {
    ALL.iter().copied().find(|i| i.key == key)
}

/// The integration a config key belongs to.
pub fn for_config_key(config_key: &str) -> Option<&'static Integration> {
    ALL.iter().copied().find(|i| i.owns(config_key))
}

impl Integration {
    pub fn owns(&self, config_key: &str) -> bool {
        match self.instances {
            Instances::Single { config_key: own } => config_key == own,
            Instances::Multi { prefix, .. } => {
                config_key.len() > prefix.len() && config_key.starts_with(prefix)
            }
        }
    }

    pub fn is_multi(&self) -> bool {
        matches!(self.instances, Instances::Multi { .. })
    }

    /// The config key of an instance; the name is checked for a
    /// multi-instance integration and ignored otherwise.
    pub fn config_key(&self, instance: Option<&str>) -> Result<String, String> {
        match self.instances {
            Instances::Single { config_key } => Ok(config_key.to_owned()),
            Instances::Multi { prefix, .. } => {
                let name = instance.unwrap_or_default().trim();
                validate_name(name)?;
                Ok(format!("{prefix}{name}"))
            }
        }
    }

    /// The instance name inside a config key: `acme` of `jira-acme`.
    pub fn instance_name<'a>(&self, config_key: &'a str) -> &'a str {
        match self.instances {
            Instances::Single { .. } => config_key,
            Instances::Multi { prefix, .. } => {
                config_key.strip_prefix(prefix).unwrap_or(config_key)
            }
        }
    }

    /// What one instance is called: "instance", "workspace", "organization".
    pub fn noun(&self) -> &'static str {
        match self.instances {
            Instances::Single { .. } => "integration",
            Instances::Multi { noun, .. } => noun,
        }
    }

    pub fn has_token_field(&self) -> bool {
        matches!(
            self.token,
            TokenSource::Field | TokenSource::FieldInHeader { .. }
        )
    }

    /// The variable a header-carried token is supplied through.
    pub fn header_env_var(&self) -> Option<&'static str> {
        match self.token {
            TokenSource::FieldInHeader { env_var } | TokenSource::GhCli { env_var } => {
                Some(env_var)
            }
            TokenSource::Field | TokenSource::None => None,
        }
    }
}

const TOKEN_FIELD: Field = Field {
    id: TOKEN,
    label: "Token",
    hint: "",
    kind: FieldKind::Secret,
};

fn value<'a>(values: &'a Values, id: &str) -> &'a str {
    values
        .get(id)
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
}

fn required(values: &Values, id: &str, label: &str) -> Result<String, String> {
    match value(values, id) {
        "" => Err(format!("{label} is required")),
        filled => Ok(filled.to_owned()),
    }
}

/// The usual answer to "is this token good": the account's name on success,
/// the status on refusal.
fn account(
    response: Result<crate::http::Response, String>,
    name: impl Fn(&serde_json::Value) -> Option<String>,
) -> Result<String, String> {
    let response = response?;
    if !response.ok() {
        return Err(format!(
            "Token invalid (HTTP {}). Check it and try again.",
            response.status
        ));
    }
    Ok(name(&response.json()).unwrap_or_else(|| "account".to_owned()))
}

fn text(json: &serde_json::Value) -> Option<String> {
    json.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

fn no_prefill(_: &ServerConfig) -> Values {
    Values::new()
}

fn no_rows(_: &ServerConfig) -> Vec<(&'static str, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_keys_find_their_integration() {
        assert_eq!(for_config_key("jira-acme").unwrap().key, "jira");
        assert_eq!(for_config_key("ado-acme").unwrap().key, "azure-devops");
        assert_eq!(for_config_key("github").unwrap().key, "github");
        assert!(
            for_config_key("jira-").is_none(),
            "a bare prefix is nobody's"
        );
        assert!(for_config_key("github-enterprise").is_none());
        assert!(for_config_key("filesystem").is_none());
    }

    #[test]
    fn instance_names_make_config_keys_and_back() {
        let jira = by_key("jira").unwrap();
        assert_eq!(jira.config_key(Some(" acme ")).unwrap(), "jira-acme");
        assert!(jira.config_key(Some("Acme Corp")).is_err());
        assert!(jira.config_key(None).is_err());
        assert_eq!(jira.instance_name("jira-client-b"), "client-b");
        let asana = by_key("asana").unwrap();
        assert_eq!(asana.config_key(Some("ignored")).unwrap(), "asana");
    }

    #[test]
    fn every_integration_is_well_formed() {
        let mut keys = std::collections::BTreeSet::new();
        for integration in ALL {
            assert!(keys.insert(integration.key), "{} twice", integration.key);
            let token_fields = integration.fields.iter().filter(|f| f.id == TOKEN).count();
            assert_eq!(
                token_fields,
                usize::from(integration.has_token_field()),
                "{}",
                integration.key
            );
            for field in integration.fields {
                assert_eq!(
                    field.kind == FieldKind::Secret,
                    field.id == TOKEN,
                    "{}",
                    field.id
                );
            }
        }
    }
}
