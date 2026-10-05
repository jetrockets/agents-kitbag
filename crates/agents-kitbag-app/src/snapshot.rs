//! What the window draws. The worker publishes a new one after everything
//! it does; a view never asks the outside world anything.

use agents_kitbag_core::assistant::Assistant;
use agents_kitbag_core::gh;
use agents_kitbag_core::health::Health;
use agents_kitbag_core::integrations::Values;
use agents_kitbag_core::platform::Os;
use agents_kitbag_core::prereq::Missing;
use agents_kitbag_core::setup::Done;
use agents_kitbag_core::storage::Available;

#[derive(Clone, Debug, PartialEq)]
pub enum HealthView {
    /// Not checked yet.
    Unknown,
    Checking,
    Known(Health),
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceView {
    /// The config key: `jira-acme`.
    pub key: String,
    /// What the detail pane shows about it (never a token).
    pub rows: Vec<(String, String)>,
    /// The form's values when this server is edited (never a token).
    pub prefill: Values,
    /// Where its token is kept; `None` for a server without one.
    pub location: Option<String>,
    pub health: HealthView,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntegrationView {
    pub key: &'static str,
    pub instances: Vec<InstanceView>,
    /// `npx` or `uvx`, when it is in none of its usual places.
    pub missing: Option<Missing>,
}

/// How the last Save ended. `id` is the form's own number for it.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveOutcome {
    pub id: u64,
    pub result: Result<Done, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Good,
    Bad,
}

/// One line about the last thing that happened.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub tone: Tone,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub os: Os,
    /// The assistant everything below is about.
    pub assistant: Assistant,
    pub config_path: String,
    /// The config exists but cannot be used: nothing is written until it is fixed.
    pub config_error: Option<String>,
    pub integrations: Vec<IntegrationView>,
    /// The stores that work here; `None` until they have been probed.
    pub stores: Option<Available>,
    pub gh: Option<gh::State>,
    /// GitHub is connected, but the CLI's token cannot reach Projects.
    pub gh_lacks_project_scope: bool,
    /// What the worker is doing that takes a while.
    pub busy: Option<String>,
    pub save: Option<SaveOutcome>,
    /// The config changed since the assistant last read it.
    pub needs_restart: bool,
    /// Servers still launched through the Node runner, or a runner that moved.
    pub migration: Vec<String>,
    pub backups: usize,
    pub notice: Option<Notice>,
    /// The runner is somewhere that will not last (a disk image, Downloads).
    pub runner_warning: Option<String>,
    pub runner_path: String,
    pub log_dir: Option<String>,
}

impl Snapshot {
    pub fn empty(os: Os) -> Self {
        Self {
            os,
            assistant: Assistant::ClaudeDesktop,
            config_path: String::new(),
            config_error: None,
            integrations: Vec::new(),
            stores: None,
            gh: None,
            gh_lacks_project_scope: false,
            busy: None,
            save: None,
            needs_restart: false,
            migration: Vec::new(),
            backups: 0,
            notice: None,
            runner_warning: None,
            runner_path: String::new(),
            log_dir: None,
        }
    }

    pub fn integration(&self, key: &str) -> Option<&IntegrationView> {
        self.integrations.iter().find(|i| i.key == key)
    }
}
