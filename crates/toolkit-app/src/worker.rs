//! The one thread that touches the outside world: the config, the stores,
//! the services, Claude Desktop. It takes `Command`s from the window and
//! publishes a `Snapshot` after everything it does.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use toolkit_core::claude::{self, Restart};
use toolkit_core::integrations::{self, TokenSource, Values};
use toolkit_core::platform::{self, Os};
use toolkit_core::setup::{self, Request};
use toolkit_core::storage::{self, StoreChoice};
use toolkit_core::{backup, gh, migrate, prereq};

use crate::backend::{self, Backend};
use crate::snapshot::{
    HealthView, InstanceView, IntegrationView, Notice, SaveOutcome, Snapshot, Tone,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Read the config again.
    Refresh,
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
    },
    Delete(Vec<String>),
    PurgeBackups,
    RestartClaude,
    /// Windows: the person restarted Claude Desktop themselves.
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
                ..
            } => {
                let store = match store {
                    StoreChoice::Gh => "the GitHub CLI",
                    StoreChoice::OnePassword { .. } => "1Password",
                    StoreChoice::System => "the system store",
                    StoreChoice::Plain => "the config file",
                };
                match instance {
                    Some(instance) => {
                        format!("save {integration} \"{instance}\", token in {store}")
                    }
                    None => format!("save {integration}, token in {store}"),
                }
            }
            Command::Check(key) => format!("check {key}"),
            Command::Delete(keys) => format!("delete {}", keys.join(", ")),
            Command::OpenUrl(url) => format!("open {url}"),
            Command::Refresh => "read the config again".to_owned(),
            Command::LoadStores => "look for credential stores".to_owned(),
            Command::CheckAll => "check every token".to_owned(),
            Command::PurgeBackups => "delete the config backups".to_owned(),
            Command::RestartClaude => "restart Claude Desktop".to_owned(),
            Command::RestartDone => "Claude Desktop was restarted by hand".to_owned(),
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
    health: HashMap<String, HealthView>,
    state: Snapshot,
    shared: Arc<Mutex<Snapshot>>,
    wake: Box<dyn Fn() + Send>,
}

impl Worker {
    pub fn new(backend: Backend, shared: Arc<Mutex<Snapshot>>, wake: Box<dyn Fn() + Send>) -> Self {
        let mut state = Snapshot::empty(backend.env.os);
        state.config_path = backend.config.path().to_string_lossy().into_owned();
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
            state,
            shared,
            wake,
        }
    }

    /// Reads the config and tells the window.
    fn publish(&mut self) {
        let servers = match self.backend.config.servers() {
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
                        health: self.health.get(key).cloned().unwrap_or(HealthView::Unknown),
                    })
                    .collect(),
                missing: (!self.backend.demo)
                    .then(|| prereq::missing(&self.backend.env, integration.launcher))
                    .flatten(),
            })
            .collect();
        self.state.migration =
            migrate::pending(&servers, &self.backend.runner_path, self.backend.env.os);
        self.state.backups = backup::count(self.backend.config.path());
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
            Command::LoadStores => self.load_stores(),
            Command::CheckAll => self.check(None),
            Command::Check(key) => self.check(Some(key)),
            Command::Save {
                id,
                integration,
                instance,
                values,
                store,
            } => {
                self.save(id, integration, instance, values, store);
            }
            Command::Delete(keys) => match self.backend.config.remove_servers(&keys) {
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
                match backup::purge(self.backend.config.path(), self.backend.env.os) {
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
                    &self.backend.config,
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
                let folder = self.backend.config.path().parent().map(Path::to_path_buf);
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
        self.state.stores = Some(stores);
        self.state.gh = Some(gh);
    }

    /// One server's health, or everyone's.
    fn check(&mut self, only: Option<String>) {
        let servers: Vec<_> = self
            .backend
            .config
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
            self.notice(
                Tone::Good,
                format!("{} connected ({})", integration.name, done.account),
            );
            self.check(Some(done.key.clone()));
        }
        self.state.save = Some(SaveOutcome { id, result });
    }

    fn restart(&mut self) {
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
    use toolkit_core::health::Status;

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
        };
        let line = save.describe();
        assert_eq!(line, "save jira \"acme\", token in 1Password");
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
