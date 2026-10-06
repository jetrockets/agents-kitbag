//! The one thread that touches the outside world: the config, the stores,
//! the services, the assistants. It takes `Command`s from the window and
//! publishes a `Snapshot` after everything it does.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use agents_kitbag_core::assistant::{Assistant, Loads};
use agents_kitbag_core::claude::{self, Restart};
use agents_kitbag_core::integrations::{self, TokenSource, Values};
use agents_kitbag_core::platform::{self, Os};
use agents_kitbag_core::setup::{self, Request};
use agents_kitbag_core::storage::{self, StoreChoice};
use agents_kitbag_core::{backup, gh, migrate, prereq};

use crate::backend::{self, Backend};
use crate::snapshot::{
    HealthView, InstanceView, IntegrationView, Notice, SaveOutcome, Snapshot, Tone,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Read the config again.
    Refresh,
    /// Look at another assistant's servers.
    UseAssistant(Assistant),
    /// Probe the credential stores and the GitHub CLI.
    LoadStores,
    CheckAll,
    Check(String),
    Save {
        id: u64,
        integration: &'static str,
        instance: Option<String>,
        values: Values,
        store: StoreChoice,
        /// The other assistants to set the same server up for.
        also: Vec<Assistant>,
    },
    Delete(Vec<String>),
    PurgeBackups,
    RestartClaude,
    /// The person restarted Claude Desktop themselves, or has read that new
    /// sessions load the change.
    RestartDone,
    Migrate,
    InstallGh,
    GhLogin,
    GhAddProjectScope,
    OpenUrl(String),
    RevealConfig,
    RevealLogs,
    DismissNotice,
}

impl Command {
    /// The command for the log: what was asked, never what was typed. A
    /// `Save` carries the form's values, the token among them, so it is
    /// never logged through `Debug`.
    pub fn describe(&self) -> String {
        match self {
            Command::Save {
                integration,
                instance,
                store,
                also,
                ..
            } => {
                let store = match store {
                    StoreChoice::Gh => "the GitHub CLI",
                    StoreChoice::OnePassword { .. } => "1Password",
                    StoreChoice::System => "the system store",
                    StoreChoice::Plain => "the config file",
                };
                let also = match also.as_slice() {
                    [] => String::new(),
                    others => format!(
                        ", also for {}",
                        others
                            .iter()
                            .map(|a| a.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                match instance {
                    Some(instance) => {
                        format!("save {integration} \"{instance}\", token in {store}{also}")
                    }
                    None => format!("save {integration}, token in {store}{also}"),
                }
            }
            Command::Check(key) => format!("check {key}"),
            Command::Delete(keys) => format!("delete {}", keys.join(", ")),
            Command::OpenUrl(url) => format!("open {url}"),
            Command::Refresh => "read the config again".to_owned(),
            Command::UseAssistant(assistant) => format!("look at {}", assistant.name()),
            Command::LoadStores => "look for credential stores".to_owned(),
            Command::CheckAll => "check every token".to_owned(),
            Command::PurgeBackups => "delete the config backups".to_owned(),
            Command::RestartClaude => "restart Claude Desktop".to_owned(),
            Command::RestartDone => "the change was acknowledged".to_owned(),
            Command::Migrate => "move servers onto the built-in runner".to_owned(),
            Command::InstallGh => "install the GitHub CLI".to_owned(),
            Command::GhLogin => "sign in to the GitHub CLI".to_owned(),
            Command::GhAddProjectScope => "add the project scope to the GitHub CLI".to_owned(),
            Command::RevealConfig => "show the config folder".to_owned(),
            Command::RevealLogs => "show the log folder".to_owned(),
            Command::DismissNotice => "dismiss the notice".to_owned(),
        }
    }
}

pub struct Worker {
    backend: Backend,
    /// The health of the servers of the assistant in view.
    health: HashMap<String, HealthView>,
    /// What is known of the assistants not in view, so coming back to one
    /// does not ask every service again.
    set_aside: HashMap<Assistant, HashMap<String, HealthView>>,
    /// The assistants not in view whose config changed since they read it.
    unread: HashSet<Assistant>,
    state: Snapshot,
    shared: Arc<Mutex<Snapshot>>,
    wake: Box<dyn Fn() + Send>,
}

impl Worker {
    pub fn new(backend: Backend, shared: Arc<Mutex<Snapshot>>, wake: Box<dyn Fn() + Send>) -> Self {
        let mut state = Snapshot::empty(backend.env.os);
        state.assistant = backend.assistant;
        state.config_path = backend.config().path().to_string_lossy().into_owned();
        state.runner_path = backend.runner_path.clone();
        state.runner_warning = (!backend.demo)
            .then(|| backend::runner_warning(&backend.runner_path))
            .flatten();
        state.log_dir = backend
            .log_dir
            .as_deref()
            .map(|dir| dir.to_string_lossy().into_owned());
        log::info!("config: {}", state.config_path);
        log::info!("runner: {}", state.runner_path);
        if let Some(warning) = &state.runner_warning {
            log::warn!("{warning}");
        }
        Self {
            backend,
            health: HashMap::new(),
            set_aside: HashMap::new(),
            unread: HashSet::new(),
            state,
            shared,
            wake,
        }
    }

    /// Reads the config and tells the window.
    fn publish(&mut self) {
        let servers = match self.backend.config().servers() {
            Ok(servers) => {
                self.state.config_error = None;
                servers
            }
            Err(error) => {
                if self.state.config_error.as_deref() != Some(error.as_str()) {
                    log::warn!("{error}");
                }
                self.state.config_error = Some(error);
                Vec::new()
            }
        };
        self.state.integrations = integrations::ALL
            .iter()
            .map(|integration| IntegrationView {
                key: integration.key,
                instances: servers
                    .iter()
                    .filter(|(key, _)| integration.owns(key))
                    .map(|(key, server)| InstanceView {
                        key: key.clone(),
                        rows: (integration.describe)(server)
                            .into_iter()
                            .map(|(label, value)| (label.to_owned(), value))
                            .collect(),
                        prefill: (integration.prefill)(server),
                        location: (integration.token != TokenSource::None)
                            .then(|| storage::location(server, self.backend.env.os).label()),
                        hosted: server.url().is_some(),
                        health: self.health.get(key).cloned().unwrap_or(HealthView::Unknown),
                    })
                    .collect(),
                missing: (!self.backend.demo)
                    .then(|| prereq::missing(&self.backend.env, integration.launcher))
                    .flatten(),
            })
            .collect();
        self.state.installed = Assistant::ALL
            .iter()
            .copied()
            .filter(|assistant| assistant.installed(&self.backend.env))
            .collect();
        self.state.migration =
            migrate::pending(&servers, &self.backend.runner_path, self.backend.env.os);
        self.state.backups = backup::count(self.backend.config().path());
        *self.shared.lock().expect("snapshot") = self.state.clone();
        (self.wake)();
    }

    /// What the person is told is what the log says.
    fn notice(&mut self, tone: Tone, text: impl Into<String>) {
        let text = text.into();
        match tone {
            Tone::Good => log::info!("{text}"),
            Tone::Bad => log::warn!("{text}"),
        }
        self.state.notice = Some(Notice { tone, text });
    }

    fn busy(&mut self, what: &str) {
        self.state.busy = Some(what.to_owned());
        self.publish();
    }

    pub fn handle(&mut self, command: Command) {
        // Not the dismissal of a notice or a re-read on focus: they would
        // drown what matters.
        if !matches!(command, Command::Refresh | Command::DismissNotice) {
            log::info!("{}", command.describe());
        }
        match command {
            Command::Refresh | Command::DismissNotice => {
                if command == Command::DismissNotice {
                    self.state.notice = None;
                }
            }
            Command::UseAssistant(assistant) => self.use_assistant(assistant),
            Command::LoadStores => self.load_stores(),
            Command::CheckAll => self.check(None),
            Command::Check(key) => self.check(Some(key)),
            Command::Save {
                id,
                integration,
                instance,
                values,
                store,
                also,
            } => {
                self.save(id, integration, instance, values, store, also);
            }
            Command::Delete(keys) => match self.backend.config().remove_servers(&keys) {
                Ok(removed) if removed.is_empty() => {}
                Ok(removed) => {
                    for key in &removed {
                        self.health.remove(key);
                    }
                    self.state.needs_restart = true;
                    self.notice(Tone::Good, format!("Deleted {}", removed.join(", ")));
                }
                Err(error) => self.notice(Tone::Bad, error),
            },
            Command::PurgeBackups => {
                match backup::purge(self.backend.config().path(), self.backend.env.os) {
                    Ok(0) => self.notice(Tone::Good, "No backups to delete"),
                    Ok(count) => self.notice(Tone::Good, format!("Deleted {count} backup(s)")),
                    Err(error) => {
                        self.notice(Tone::Bad, format!("Could not delete the backups: {error}"))
                    }
                }
            }
            Command::RestartClaude => self.restart(),
            Command::RestartDone => self.state.needs_restart = false,
            Command::Migrate => {
                match migrate::migrate(
                    self.backend.config(),
                    &self.backend.runner_path,
                    self.backend.env.os,
                ) {
                    Ok(keys) if keys.is_empty() => {}
                    Ok(keys) => {
                        self.state.needs_restart = true;
                        self.notice(
                            Tone::Good,
                            format!("Moved {} onto the built-in runner", keys.join(", ")),
                        );
                    }
                    Err(error) => self.notice(Tone::Bad, error),
                }
            }
            Command::InstallGh => {
                self.busy("Installing the GitHub CLI...");
                let installed = self.backend.with(|ctx| ctx.gh().install());
                self.state.busy = None;
                match installed {
                    Ok(()) => self.notice(Tone::Good, "GitHub CLI installed"),
                    Err(error) => self.notice(Tone::Bad, error),
                }
                self.load_stores();
            }
            Command::GhLogin => {
                let command = self.backend.with(|ctx| ctx.gh().login_command());
                self.in_terminal(
                    &command,
                    "Finish signing in in the terminal window, then check again.",
                );
            }
            Command::GhAddProjectScope => {
                let command = self
                    .backend
                    .with(|ctx| ctx.gh().add_project_scope_command());
                self.in_terminal(
                    &command,
                    "Approve the scope in the terminal window, then connect GitHub again.",
                );
            }
            Command::OpenUrl(url) => {
                if !self.backend.demo {
                    platform::open_url(self.backend.env.os, &url);
                }
                return;
            }
            Command::RevealConfig => {
                let folder = self.backend.config().path().parent().map(Path::to_path_buf);
                self.reveal(folder.as_deref());
                return;
            }
            Command::RevealLogs => {
                let folder = self.backend.log_dir.clone();
                self.reveal(folder.as_deref());
                return;
            }
        }
        self.publish();
    }

    /// Turns to another assistant. What was known of the one left behind is
    /// kept; one seen for the first time has its tokens checked.
    fn use_assistant(&mut self, assistant: Assistant) {
        let left = self.backend.assistant;
        if assistant == left {
            return;
        }
        self.set_aside
            .insert(left, std::mem::take(&mut self.health));
        if std::mem::take(&mut self.state.needs_restart) {
            self.unread.insert(left);
        }
        self.backend.assistant = assistant;
        self.state.assistant = assistant;
        self.state.config_path = self.backend.config().path().to_string_lossy().into_owned();
        self.state.config_error = None;
        self.state.needs_restart = self.unread.remove(&assistant);
        log::info!("config: {}", self.state.config_path);
        match self.set_aside.remove(&assistant) {
            Some(health) => self.health = health,
            None => self.check(None),
        }
    }

    fn reveal(&self, folder: Option<&Path>) {
        if let Some(folder) = folder
            && !self.backend.demo
        {
            platform::reveal(self.backend.env.os, folder);
        }
    }

    fn in_terminal(&mut self, command: &str, then: &str) {
        if self.backend.demo {
            self.notice(Tone::Good, then);
            return;
        }
        match gh::open_in_terminal(self.backend.env.os, command) {
            Ok(()) => self.notice(Tone::Good, then),
            Err(error) => self.notice(
                Tone::Bad,
                format!("Could not open a terminal ({error}). Run: {command}"),
            ),
        }
    }

    fn load_stores(&mut self) {
        let (stores, gh) = self
            .backend
            .with(|ctx| (storage::available(ctx.env, ctx.runner), ctx.gh().state()));
        log::info!(
            "stores: system {}, 1Password {}, GitHub CLI {:?}",
            stores
                .system
                .map_or("not usable", |backend| backend.label()),
            match (&stores.vaults, stores.op_unreachable) {
                (Some(vaults), _) => format!("{} vault(s)", vaults.len()),
                (None, true) => "installed but not reachable".to_owned(),
                (None, false) => "not installed".to_owned(),
            },
            gh
        );
        // The system store is only of use through the runner: without it a
        // Save would write a path to a program that is not there.
        let mut stores = stores;
        if !self.backend.demo && !Path::new(&self.backend.runner_path).exists() {
            stores.system = None;
        }
        self.state.stores = Some(stores);
        self.state.gh = Some(gh);
    }

    /// One server's health, or everyone's.
    fn check(&mut self, only: Option<String>) {
        let servers: Vec<_> = self
            .backend
            .config()
            .servers()
            .unwrap_or_default()
            .into_iter()
            .filter(|(key, _)| integrations::for_config_key(key).is_some())
            .filter(|(key, _)| only.as_ref().is_none_or(|only| only == key))
            .collect();
        if servers.is_empty() {
            return;
        }
        for (key, _) in &servers {
            self.health.insert(key.clone(), HealthView::Checking);
        }
        self.publish();
        let results = self.backend.with(|ctx| ctx.health().check_all(&servers));
        if only.is_none() {
            // Health of servers that are gone is forgotten.
            self.health.clear();
        }
        for (key, health) in results {
            log::info!(
                "{key}: {:?}{}",
                health.status,
                health
                    .detail
                    .as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            );
            self.health.insert(key, HealthView::Known(health));
        }
    }

    fn save(
        &mut self,
        id: u64,
        integration: &'static str,
        instance: Option<String>,
        values: Values,
        store: StoreChoice,
        also: Vec<Assistant>,
    ) {
        let Some(integration) = integrations::by_key(integration) else {
            return;
        };
        self.busy(match (self.backend.env.os, integration.launcher) {
            (Os::Windows, integrations::Launcher::Npx) => {
                "Checking and installing the server. The first time takes a minute..."
            }
            _ => "Checking with the service...",
        });
        let request = Request {
            integration,
            instance,
            values,
            store,
        };
        let result = self.backend.with(|ctx| setup::run(ctx, &request));
        self.state.busy = None;
        match &result {
            Ok(done) => {
                if let Some(warning) = &done.warning {
                    log::warn!("{warning}");
                }
            }
            Err(error) => log::warn!("{} was not saved: {error}", integration.name),
        }
        if let Ok(done) = &result {
            if integration.key == "github" {
                self.state.gh_lacks_project_scope = done.warning.is_some();
            }
            self.state.needs_restart = true;
            // The same server for the other assistants that were asked for.
            // Each gets a token entry of its own, so one can be deleted
            // without the others losing theirs.
            let mut reached = vec![self.backend.assistant.name()];
            let mut failed = Vec::new();
            for other in also {
                if other == self.backend.assistant || reached.contains(&other.name()) {
                    continue;
                }
                match self
                    .backend
                    .with_for(other, |ctx| setup::run(ctx, &request))
                {
                    Ok(_) => {
                        reached.push(other.name());
                        self.unread.insert(other);
                        // Its tokens are checked again when it is next in view.
                        self.set_aside.remove(&other);
                    }
                    Err(error) => {
                        log::warn!(
                            "{} was not saved for {}: {error}",
                            integration.name,
                            other.name()
                        );
                        failed.push(format!("{}: {error}", other.name()));
                    }
                }
            }
            let connected = if reached.len() == 1 {
                format!("{} connected ({})", integration.name, done.account)
            } else {
                format!(
                    "{} connected ({}) for {}",
                    integration.name,
                    done.account,
                    reached.join(", ")
                )
            };
            if failed.is_empty() {
                self.notice(Tone::Good, connected);
            } else {
                self.notice(
                    Tone::Bad,
                    format!("{connected}. Not set up for {}", failed.join("; ")),
                );
            }
            self.check(Some(done.key.clone()));
        }
        self.state.save = Some(SaveOutcome { id, result });
    }

    fn restart(&mut self) {
        if self.backend.assistant.loads() == Loads::InNewSessions {
            self.state.needs_restart = false;
            return;
        }
        self.busy("Restarting Claude Desktop...");
        let outcome = self
            .backend
            .with(|ctx| claude::restart(ctx.runner, ctx.env.os));
        self.state.busy = None;
        match outcome {
            Restart::Restarted => {
                self.state.needs_restart = false;
                self.notice(Tone::Good, "Claude Desktop restarted");
            }
            Restart::NotRunning => {
                self.state.needs_restart = false;
                self.notice(
                    Tone::Good,
                    "Claude Desktop is not running. The servers load when it next opens.",
                );
            }
            // Still to be restarted: the reminder stays.
            Restart::DidNotQuit => self.notice(
                Tone::Bad,
                "Claude Desktop did not quit. Quit it yourself (Cmd+Q) and open it again.",
            ),
            Restart::Manual => log::info!("Claude Desktop is restarted by hand on this system"),
        }
    }
}

/// The window's end of the worker.
pub struct Handle {
    commands: Sender<Command>,
    shared: Arc<Mutex<Snapshot>>,
}

impl Handle {
    /// Starts the worker; it reads the config and checks every token before
    /// it waits for the first command.
    pub fn spawn(backend: Backend, wake: Box<dyn Fn() + Send>) -> Self {
        let shared = Arc::new(Mutex::new(Snapshot::empty(backend.env.os)));
        let (commands, inbox) = channel();
        let mut worker = Worker::new(backend, Arc::clone(&shared), wake);
        worker.publish();
        std::thread::Builder::new()
            .name("worker".to_owned())
            .spawn(move || run(worker, inbox))
            .expect("the worker thread");
        Self { commands, shared }
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            log::error!("the worker is gone");
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().expect("snapshot").clone()
    }
}

/// The stores are not probed here: asking 1Password for its vaults can raise
/// its unlock prompt, which belongs to the moment a form asks where to keep a
/// token, not to opening the window.
fn run(mut worker: Worker, inbox: Receiver<Command>) {
    worker.handle(Command::CheckAll);
    while let Ok(command) = inbox.recv() {
        worker.handle(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use agents_kitbag_core::health::Status;

    fn worker() -> (Worker, Arc<Mutex<Snapshot>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let backend = demo::backend(dir.path());
        let shared = Arc::new(Mutex::new(Snapshot::empty(Os::Mac)));
        let worker = Worker::new(backend, Arc::clone(&shared), Box::new(|| {}));
        (worker, shared, dir)
    }

    fn status(snapshot: &Snapshot, integration: &str, key: &str) -> Option<Status> {
        let instance = snapshot
            .integration(integration)?
            .instances
            .iter()
            .find(|i| i.key == key)?;
        match &instance.health {
            HealthView::Known(health) => Some(health.status),
            _ => None,
        }
    }

    #[test]
    fn the_first_snapshot_lists_what_is_configured() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Refresh);
        let snapshot = shared.lock().unwrap().clone();
        let keys = |integration: &str| -> Vec<String> {
            snapshot
                .integration(integration)
                .unwrap()
                .instances
                .iter()
                .map(|i| i.key.clone())
                .collect()
        };
        assert_eq!(keys("jira"), ["jira-acme"]);
        assert_eq!(keys("linear"), ["linear-nest"]);
        assert!(keys("notion").is_empty());
        let jira = &snapshot.integration("jira").unwrap().instances[0];
        assert_eq!(
            jira.rows[0],
            ("URL".to_owned(), "https://acme.atlassian.net".to_owned())
        );
        assert_eq!(jira.location.as_deref(), Some("macOS Keychain"));
        assert_eq!(jira.health, HealthView::Unknown);
    }

    #[test]
    fn another_assistant_has_servers_and_a_config_of_its_own() {
        let (mut worker, shared, dir) = worker();
        worker.handle(Command::CheckAll);
        worker.handle(Command::UseAssistant(Assistant::Codex));
        let snapshot = shared.lock().unwrap().clone();
        assert_eq!(snapshot.assistant, Assistant::Codex);
        assert!(Path::new(&snapshot.config_path).ends_with(".codex/config.toml"));
        assert!(snapshot.integration("jira").unwrap().instances.is_empty());
        // Seen for the first time, its tokens are checked.
        assert_eq!(status(&snapshot, "figma", "figma"), Some(Status::Ok));

        worker.handle(Command::Delete(vec!["figma".into()]));
        let snapshot = shared.lock().unwrap().clone();
        assert!(snapshot.needs_restart);
        let text = std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
        assert!(!text.contains("figma"), "{text}");

        // Claude Desktop was not touched, and what was known of it is kept.
        worker.handle(Command::UseAssistant(Assistant::ClaudeDesktop));
        let snapshot = shared.lock().unwrap().clone();
        assert!(!snapshot.needs_restart);
        assert_eq!(status(&snapshot, "jira", "jira-acme"), Some(Status::Ok));
        assert!(snapshot.config_path.ends_with("claude_desktop_config.json"));

        // Codex still has its change to take in.
        worker.handle(Command::UseAssistant(Assistant::Codex));
        assert!(shared.lock().unwrap().needs_restart);
        worker.handle(Command::RestartClaude);
        assert!(!shared.lock().unwrap().needs_restart);
    }

    #[test]
    fn a_server_saved_for_claude_code_names_its_transport_and_its_own_secret() {
        let (mut worker, shared, dir) = worker();
        worker.handle(Command::UseAssistant(Assistant::ClaudeCode));
        worker.handle(Command::Save {
            id: 1,
            integration: "asana",
            instance: None,
            values: [("token".to_owned(), "tok".to_owned())]
                .into_iter()
                .collect(),
            store: StoreChoice::System,
            also: Vec::new(),
        });
        let snapshot = shared.lock().unwrap().clone();
        assert!(snapshot.save.unwrap().result.is_ok());
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(".claude.json")).unwrap())
                .unwrap();
        let asana = &config["mcpServers"]["asana"];
        assert_eq!(asana["type"], "stdio");
        assert_eq!(
            asana["args"][1],
            "ASANA_ACCESS_TOKEN=keychain:agents-kitbag-claude-code-asana"
        );
        assert_eq!(config["mcpServers"]["notion-acme"]["type"], "stdio");
    }

    #[test]
    fn one_save_sets_a_server_up_for_every_assistant_asked_for() {
        let (mut worker, shared, dir) = worker();
        worker.handle(Command::Save {
            id: 3,
            integration: "asana",
            instance: None,
            values: [("token".to_owned(), "tok".to_owned())]
                .into_iter()
                .collect(),
            store: StoreChoice::System,
            also: vec![Assistant::ClaudeCode, Assistant::Codex],
        });
        let snapshot = shared.lock().unwrap().clone();
        assert_eq!(snapshot.assistant, Assistant::ClaudeDesktop);
        assert_eq!(
            snapshot.notice.unwrap().text,
            "Asana connected (user: Ada Lovelace) for Claude Desktop, Claude Code, Codex"
        );
        assert!(snapshot.needs_restart);

        // Each assistant has the server, with a token entry of its own.
        let desktop =
            std::fs::read_to_string(dir.path().join("claude_desktop_config.json")).unwrap();
        assert!(desktop.contains("ASANA_ACCESS_TOKEN=keychain:agents-kitbag-asana\""));
        let code = std::fs::read_to_string(dir.path().join(".claude.json")).unwrap();
        assert!(code.contains("keychain:agents-kitbag-claude-code-asana"));
        let codex = std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
        assert!(codex.contains("[mcp_servers.asana]"));
        assert!(codex.contains("keychain:agents-kitbag-codex-asana"));

        // Each of the others still has the change to take in.
        worker.handle(Command::UseAssistant(Assistant::Codex));
        let snapshot = shared.lock().unwrap().clone();
        assert!(snapshot.needs_restart);
        assert_eq!(status(&snapshot, "asana", "asana"), Some(Status::Ok));
    }

    #[test]
    fn the_snapshot_says_which_assistants_are_on_this_machine() {
        let (mut worker, shared, dir) = worker();
        worker.handle(Command::Refresh);
        assert_eq!(shared.lock().unwrap().installed, Assistant::ALL);
        std::fs::remove_dir_all(dir.path().join(".codex")).unwrap();
        worker.handle(Command::Refresh);
        assert_eq!(
            shared.lock().unwrap().installed,
            [Assistant::ClaudeDesktop, Assistant::ClaudeCode]
        );
        // Nothing of the machine the tests run on is looked at.
        std::fs::remove_file(dir.path().join(".claude.json")).unwrap();
        worker.handle(Command::Refresh);
        assert_eq!(shared.lock().unwrap().installed, [Assistant::ClaudeDesktop]);
    }

    #[test]
    fn checking_everything_finds_the_expired_token() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::CheckAll);
        let snapshot = shared.lock().unwrap().clone();
        assert_eq!(status(&snapshot, "jira", "jira-acme"), Some(Status::Ok));
        assert_eq!(status(&snapshot, "linear", "linear-nest"), Some(Status::Ok));
        assert_eq!(status(&snapshot, "github", "github"), Some(Status::Ok));
        assert_eq!(status(&snapshot, "asana", "asana"), Some(Status::Expired));
    }

    #[test]
    fn a_save_writes_the_server_checks_it_and_asks_for_a_restart() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Save {
            id: 7,
            integration: "figma",
            instance: None,
            values: [("token".to_owned(), "figd_demo".to_owned())].into(),
            store: StoreChoice::System,
            also: Vec::new(),
        });
        let snapshot = shared.lock().unwrap().clone();
        let outcome = snapshot.save.clone().unwrap();
        assert_eq!(outcome.id, 7);
        assert_eq!(outcome.result.unwrap().key, "figma");
        assert!(snapshot.needs_restart);
        assert_eq!(snapshot.busy, None);
        assert_eq!(status(&snapshot, "figma", "figma"), Some(Status::Ok));
        assert_eq!(
            snapshot.integration("figma").unwrap().instances[0]
                .location
                .as_deref(),
            Some("macOS Keychain")
        );
    }

    #[test]
    fn a_refused_token_is_reported_and_nothing_changes() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Save {
            id: 1,
            integration: "notion",
            instance: Some("acme".to_owned()),
            values: [("token".to_owned(), demo::BAD_TOKEN.to_owned())].into(),
            store: StoreChoice::Plain,
            also: Vec::new(),
        });
        let snapshot = shared.lock().unwrap().clone();
        assert!(
            snapshot
                .save
                .clone()
                .unwrap()
                .result
                .unwrap_err()
                .contains("HTTP 401")
        );
        assert!(!snapshot.needs_restart);
        assert!(snapshot.integration("notion").unwrap().instances.is_empty());
    }

    #[test]
    fn deleting_removes_the_server_and_asks_for_a_restart() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Delete(vec!["jira-acme".to_owned()]));
        let snapshot = shared.lock().unwrap().clone();
        assert!(snapshot.integration("jira").unwrap().instances.is_empty());
        assert!(snapshot.needs_restart);
        assert_eq!(snapshot.notice.unwrap().text, "Deleted jira-acme");
    }

    #[test]
    fn restarting_clears_the_reminder() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Delete(vec!["asana".to_owned()]));
        worker.handle(Command::RestartClaude);
        let snapshot = shared.lock().unwrap().clone();
        assert!(!snapshot.needs_restart);
        assert_eq!(snapshot.notice.unwrap().text, "Claude Desktop restarted");
    }

    #[test]
    fn the_node_runner_is_offered_for_migration_and_moved() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Refresh);
        assert_eq!(shared.lock().unwrap().migration, ["linear-nest"]);
        worker.handle(Command::Migrate);
        let snapshot = shared.lock().unwrap().clone();
        assert!(snapshot.migration.is_empty());
        assert!(snapshot.needs_restart);
    }

    #[test]
    fn backups_are_counted_and_purged() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::Delete(vec!["asana".to_owned()]));
        assert_eq!(shared.lock().unwrap().backups, 1);
        worker.handle(Command::PurgeBackups);
        let snapshot = shared.lock().unwrap().clone();
        assert_eq!(snapshot.backups, 0);
        assert_eq!(snapshot.notice.unwrap().text, "Deleted 1 backup(s)");
    }

    #[test]
    fn the_stores_and_the_github_cli_are_probed() {
        let (mut worker, shared, _dir) = worker();
        worker.handle(Command::LoadStores);
        let snapshot = shared.lock().unwrap().clone();
        let stores = snapshot.stores.unwrap();
        assert!(stores.gh);
        assert_eq!(stores.vaults.unwrap(), ["Private", "Work"]);
        assert_eq!(snapshot.gh, Some(gh::State::Ready));
    }

    #[test]
    fn a_config_that_does_not_parse_is_shown_not_overwritten() {
        let (mut worker, shared, dir) = worker();
        std::fs::write(dir.path().join("claude_desktop_config.json"), "{ nope").unwrap();
        worker.handle(Command::Save {
            id: 2,
            integration: "figma",
            instance: None,
            values: [("token".to_owned(), "figd_demo".to_owned())].into(),
            store: StoreChoice::Plain,
            also: Vec::new(),
        });
        let snapshot = shared.lock().unwrap().clone();
        assert!(snapshot.config_error.unwrap().contains("not valid JSON"));
        assert!(snapshot.save.unwrap().result.is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("claude_desktop_config.json")).unwrap(),
            "{ nope"
        );
    }

    #[test]
    fn a_save_is_logged_without_what_was_typed() {
        let save = Command::Save {
            id: 1,
            integration: "jira",
            instance: Some("acme".to_owned()),
            values: [
                ("token".to_owned(), "s3cret-token".to_owned()),
                ("email".to_owned(), "ada@acme.com".to_owned()),
            ]
            .into(),
            store: StoreChoice::OnePassword {
                vault: "Private".to_owned(),
            },
            also: vec![Assistant::ClaudeCode, Assistant::Codex],
        };
        let line = save.describe();
        assert_eq!(
            line,
            "save jira \"acme\", token in 1Password, also for Claude Code, Codex"
        );
        assert!(!line.contains("s3cret-token") && !line.contains("ada@acme.com"));
        assert_eq!(
            Command::Check("jira-acme".into()).describe(),
            "check jira-acme"
        );
        assert_eq!(
            Command::Delete(vec!["a".into(), "b".into()]).describe(),
            "delete a, b"
        );
    }
}
