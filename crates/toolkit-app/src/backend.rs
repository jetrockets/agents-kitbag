//! The doors to the outside, owned: the real ones for the app, made-up ones
//! for `--demo` and the tests.

use std::path::{Path, PathBuf};

use toolkit_core::config::ConfigFile;
use toolkit_core::exec::{CommandRunner, SystemRunner};
use toolkit_core::http::{Http, UreqHttp};
use toolkit_core::packages::SystemPackages;
use toolkit_core::platform::{Env, Os};
use toolkit_core::runner;
use toolkit_core::setup::Ctx;

pub struct Backend {
    pub env: Env,
    pub runner: Box<dyn CommandRunner>,
    pub http: Box<dyn Http>,
    pub config: ConfigFile,
    pub runner_path: String,
    pub log_dir: Option<PathBuf>,
    /// `--demo`: nothing leaves the app, so nothing opens a browser either.
    pub demo: bool,
}

impl Backend {
    pub fn real(log_dir: Option<PathBuf>) -> anyhow::Result<Self> {
        let env = Env::current();
        Ok(Self {
            config: ConfigFile::locate(&env),
            runner: Box::new(SystemRunner),
            http: Box::new(UreqHttp::new().map_err(anyhow::Error::msg)?),
            runner_path: runner_path(env.os).to_string_lossy().into_owned(),
            env,
            log_dir,
            demo: false,
        })
    }

    /// Runs `work` with everything `toolkit-core` reaches the outside through.
    pub fn with<R>(&self, work: impl FnOnce(&Ctx) -> R) -> R {
        let packages = SystemPackages {
            env: &self.env,
            runner: &*self.runner,
        };
        work(&Ctx {
            env: &self.env,
            runner: &*self.runner,
            http: &*self.http,
            packages: &packages,
            config: &self.config,
            runner_path: &self.runner_path,
        })
    }
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
        let downloads = dir.path().join("Downloads").join("Claude Toolkit");
        std::fs::create_dir_all(&downloads).unwrap();
        let runner = downloads.join("claude-toolkit-runner");
        std::fs::write(&runner, "").unwrap();
        assert!(
            runner_warning(&runner.to_string_lossy())
                .unwrap()
                .contains("temporary")
        );

        let installed = dir.path().join("Applications");
        std::fs::create_dir_all(&installed).unwrap();
        let runner = installed.join("claude-toolkit-runner");
        std::fs::write(&runner, "").unwrap();
        assert_eq!(runner_warning(&runner.to_string_lossy()), None);

        assert!(
            runner_warning("/nowhere/claude-toolkit-runner")
                .unwrap()
                .contains("missing")
        );
    }
}
