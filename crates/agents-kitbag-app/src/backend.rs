//! The doors to the outside, owned: the real ones for the app, made-up ones
//! for `--demo` and the tests.

use std::path::{Path, PathBuf};

use agents_kitbag_core::assistant::Assistant;
use agents_kitbag_core::config::ConfigFile;
use agents_kitbag_core::exec::{CommandRunner, SystemRunner};
use agents_kitbag_core::http::{Http, UreqHttp};
use agents_kitbag_core::packages::SystemPackages;
use agents_kitbag_core::platform::{Env, Os};
use agents_kitbag_core::runner;
use agents_kitbag_core::setup::Ctx;

pub struct Backend {
    pub env: Env,
    pub runner: Box<dyn CommandRunner>,
    pub http: Box<dyn Http>,
    /// The config of each assistant in [`Assistant::ALL`].
    pub configs: Vec<ConfigFile>,
    /// The assistant the window is looking at.
    pub assistant: Assistant,
    pub runner_path: String,
    pub log_dir: Option<PathBuf>,
    /// `--demo`: nothing leaves the app, so nothing opens a browser either.
    pub demo: bool,
}

impl Backend {
    pub fn real(log_dir: Option<PathBuf>) -> anyhow::Result<Self> {
        let env = Env::current();
        // The first assistant that is on this machine, so the window does
        // not open on one that is not.
        let assistant = Assistant::ALL
            .iter()
            .copied()
            .find(|assistant| assistant.installed(&env))
            .unwrap_or(Assistant::ClaudeDesktop);
        Ok(Self {
            configs: configs(&env),
            assistant,
            runner: Box::new(SystemRunner),
            http: Box::new(UreqHttp::new().map_err(anyhow::Error::msg)?),
            runner_path: runner_path(env.os).to_string_lossy().into_owned(),
            env,
            log_dir,
            demo: false,
        })
    }

    /// The config of the assistant the window is looking at.
    pub fn config(&self) -> &ConfigFile {
        self.config_of(self.assistant)
    }

    pub fn config_of(&self, assistant: Assistant) -> &ConfigFile {
        self.configs
            .iter()
            .find(|config| config.assistant() == assistant)
            .expect("every assistant has a config")
    }

    /// Runs `work` with everything `agents-kitbag-core` reaches the outside through.
    pub fn with<R>(&self, work: impl FnOnce(&Ctx) -> R) -> R {
        self.with_for(self.assistant, work)
    }

    /// The same, writing to this assistant's config.
    pub fn with_for<R>(&self, assistant: Assistant, work: impl FnOnce(&Ctx) -> R) -> R {
        let packages = SystemPackages {
            env: &self.env,
            runner: &*self.runner,
        };
        work(&Ctx {
            env: &self.env,
            runner: &*self.runner,
            http: &*self.http,
            packages: &packages,
            config: self.config_of(assistant),
            runner_path: &self.runner_path,
        })
    }
}

/// Where each assistant keeps its servers on this machine.
pub fn configs(env: &Env) -> Vec<ConfigFile> {
    Assistant::ALL
        .iter()
        .map(|assistant| assistant.config(env))
        .collect()
}

/// The runner sits beside the app: in `Contents/MacOS` of the bundle, or in
/// the app's folder on Windows.
fn runner_path(os: Os) -> PathBuf {
    let name = match os {
        Os::Windows => format!("{}.exe", runner::NAME),
        Os::Mac | Os::Linux => runner::NAME.to_owned(),
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join(name)
}

/// Why a runner path written into the config now would not last, if it would not.
pub fn runner_warning(runner_path: &str) -> Option<String> {
    let path = Path::new(runner_path);
    if !path.exists() {
        return Some(format!(
            "The runner that starts servers with stored tokens is missing ({runner_path}). Tokens can only be kept in the config file or 1Password."
        ));
    }
    let text = runner_path.replace('\\', "/");
    let temporary = [
        "/Volumes/",
        "/AppTranslocation/",
        "/Downloads/",
        "/target/debug/",
        "/target/release/",
    ]
    .iter()
    .any(|part| text.contains(part));
    temporary.then(|| {
        "This copy runs from a temporary place. Move the app to Applications (or a permanent folder) before storing tokens: the config will point at the runner beside it.".to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runner_in_a_temporary_place_is_warned_about() {
        let dir = tempfile::tempdir().unwrap();
        let downloads = dir.path().join("Downloads").join("Agents Kitbag");
        std::fs::create_dir_all(&downloads).unwrap();
        let runner = downloads.join("agents-kitbag-runner");
        std::fs::write(&runner, "").unwrap();
        assert!(
            runner_warning(&runner.to_string_lossy())
                .unwrap()
                .contains("temporary")
        );

        let installed = dir.path().join("Applications");
        std::fs::create_dir_all(&installed).unwrap();
        let runner = installed.join("agents-kitbag-runner");
        std::fs::write(&runner, "").unwrap();
        assert_eq!(runner_warning(&runner.to_string_lossy()), None);

        assert!(
            runner_warning("/nowhere/agents-kitbag-runner")
                .unwrap()
                .contains("missing")
        );
    }
}
