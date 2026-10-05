//! The assistants whose setup the app looks after. Each keeps its MCP servers
//! in a file of its own, in a format of its own.

use std::path::PathBuf;

use crate::config::{self, ConfigFile};
use crate::platform::Env;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Assistant {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
}

/// When an assistant takes in a change to its config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loads {
    /// The app reads its config when it starts.
    OnRestart,
    /// Every new session reads the config; one that is open keeps what it has.
    InNewSessions,
}

impl Assistant {
    pub const ALL: &[Assistant] = &[
        Assistant::ClaudeDesktop,
        Assistant::ClaudeCode,
        Assistant::Codex,
    ];

    /// The name for the command line and the log.
    pub fn key(self) -> &'static str {
        match self {
            Assistant::ClaudeDesktop => "claude-desktop",
            Assistant::ClaudeCode => "claude-code",
            Assistant::Codex => "codex",
        }
    }

    pub fn by_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|a| a.key() == key)
    }

    pub fn name(self) -> &'static str {
        match self {
            Assistant::ClaudeDesktop => "Claude Desktop",
            Assistant::ClaudeCode => "Claude Code",
            Assistant::Codex => "Codex",
        }
    }

    pub fn loads(self) -> Loads {
        match self {
            Assistant::ClaudeDesktop => Loads::OnRestart,
            Assistant::ClaudeCode | Assistant::Codex => Loads::InNewSessions,
        }
    }

    /// Where the assistant reads its servers from on this machine.
    ///
    /// Claude Code keeps the servers of every project ("user scope") in
    /// `.claude.json`, in the home folder or in `CLAUDE_CONFIG_DIR`. Codex
    /// keeps them in `config.toml` under `~/.codex` or `CODEX_HOME`. Both
    /// variables are read from the app's own environment: one set only in a
    /// shell profile does not reach an app started from the desktop.
    pub fn config_path(self, env: &Env) -> PathBuf {
        match self {
            Assistant::ClaudeDesktop => config::resolve_path(env),
            Assistant::ClaudeCode => env
                .var("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| env.home.clone())
                .join(".claude.json"),
            Assistant::Codex => env
                .var("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| env.home.join(".codex"))
                .join("config.toml"),
        }
    }

    pub fn config(self, env: &Env) -> ConfigFile {
        ConfigFile::of(self, self.config_path(env), env.os)
    }

    /// The name a server's token is stored under. Two assistants can each
    /// have a `jira-acme`; deleting one must not take the other's token.
    /// Claude Desktop keeps the bare key, as stored tokens already have it.
    pub fn secret_key(self, config_key: &str) -> String {
        match self {
            Assistant::ClaudeDesktop => config_key.to_owned(),
            Assistant::ClaudeCode | Assistant::Codex => format!("{}-{config_key}", self.key()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;

    #[test]
    fn each_assistant_has_its_own_file() {
        let env = Env::with(Os::Mac, "/Users/ada", &[]);
        assert_eq!(
            Assistant::ClaudeCode.config_path(&env),
            PathBuf::from("/Users/ada/.claude.json")
        );
        assert_eq!(
            Assistant::Codex.config_path(&env),
            PathBuf::from("/Users/ada/.codex/config.toml")
        );
        assert!(
            Assistant::ClaudeDesktop
                .config_path(&env)
                .ends_with("Claude/claude_desktop_config.json")
        );
    }

    #[test]
    fn the_assistants_own_variables_move_their_files() {
        let env = Env::with(
            Os::Linux,
            "/home/ada",
            &[
                ("CLAUDE_CONFIG_DIR", "/cfg/claude"),
                ("CODEX_HOME", "/cfg/codex"),
            ],
        );
        assert_eq!(
            Assistant::ClaudeCode.config_path(&env),
            PathBuf::from("/cfg/claude/.claude.json")
        );
        assert_eq!(
            Assistant::Codex.config_path(&env),
            PathBuf::from("/cfg/codex/config.toml")
        );
    }

    #[test]
    fn tokens_of_two_assistants_do_not_share_a_name() {
        assert_eq!(
            Assistant::ClaudeDesktop.secret_key("jira-acme"),
            "jira-acme"
        );
        assert_eq!(
            Assistant::ClaudeCode.secret_key("jira-acme"),
            "claude-code-jira-acme"
        );
        assert_eq!(Assistant::Codex.secret_key("jira-acme"), "codex-jira-acme");
    }

    #[test]
    fn keys_round_trip() {
        for assistant in Assistant::ALL {
            assert_eq!(Assistant::by_key(assistant.key()), Some(*assistant));
        }
        assert_eq!(Assistant::by_key("cursor"), None);
    }
}
