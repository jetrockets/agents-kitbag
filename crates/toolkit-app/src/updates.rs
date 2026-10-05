//! Self-update through fastframe-update, from the repository's GitHub
//! releases.
//!
//! A newer release is offered in the sidebar and nothing is downloaded until
//! the person clicks. A check that fails (no network) is logged and tried
//! again later.
//!
//! A release is installed only when its `checksums.txt` carries a valid
//! signature by the key compiled in here. See packaging/UPDATE_SIGNING.md.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use eframe::egui;
use fastframe_update::{MacConfig, Release, ReqwestTransport, UpdateConfig, Updater};

/// How often to look for a newer release.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub const CONFIG: UpdateConfig = UpdateConfig {
    macos: MacConfig {
        bundle_ids: &["com.jetrockets.claude-toolkit"],
        executable_names: &[],
        legacy_bundle_names: &[],
    },
    publisher_key: Some(include_str!("../assets/update-public-key.hex")),
    ..UpdateConfig::new(
        "jetrockets/claude-toolkit",
        "Claude Toolkit",
        "claude-toolkit",
        env!("CARGO_PKG_VERSION"),
    )
};

fn updater() -> anyhow::Result<Updater> {
    let transport = ReqwestTransport::new(reqwest::blocking::Client::builder())?;
    Ok(Updater::new(CONFIG, transport))
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Idle,
    /// A newer release, not downloaded.
    Available {
        version: String,
    },
    Downloading {
        version: String,
    },
    /// Handed to fastframe's helper: the app quits so it can install.
    Installing,
    Failed {
        version: String,
    },
}

/// What the window shows about an update.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub version: String,
    pub downloading: bool,
    pub failed: bool,
}

/// How the last look for a newer release ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Checked {
    /// Not looked yet (or updates are off, as in `--demo`).
    #[default]
    Never,
    Checking,
    /// This is the newest release.
    UpToDate,
    /// A newer release this copy can install: see the [`Offer`].
    Available,
    /// A newer release this copy will not replace itself with, and why
    /// (run from the disk image, owned by a package manager).
    Elsewhere {
        version: String,
        reason: String,
    },
    /// No answer: no network, or GitHub said no.
    Unreachable(String),
}

/// What one look found.
enum Found {
    Newer(Release),
    Nothing,
    Elsewhere { version: String, reason: String },
    Unreachable(String),
}

pub struct Updates {
    phase: Arc<Mutex<Phase>>,
    release: Arc<Mutex<Option<Release>>>,
    checked: Arc<Mutex<Checked>>,
    ctx: Option<egui::Context>,
}

impl Updates {
    /// No checks: `--demo`.
    pub fn disabled() -> Self {
        Self {
            phase: Arc::new(Mutex::new(Phase::Idle)),
            release: Arc::new(Mutex::new(None)),
            checked: Arc::new(Mutex::new(Checked::Never)),
            ctx: None,
        }
    }

    /// Looks at once, and again every six hours.
    pub fn spawn(ctx: egui::Context) -> Self {
        let updates = Self {
            ctx: Some(ctx),
            ..Self::disabled()
        };
        let looker = updates.looker();
        thread::spawn(move || {
            if let Some(folder) = installation_folder() {
                clean_staging(&folder, SystemTime::now());
            }
            loop {
                looker.look();
                thread::sleep(CHECK_EVERY);
            }
        });
        updates
    }

    fn looker(&self) -> Looker {
        Looker {
            phase: Arc::clone(&self.phase),
            release: Arc::clone(&self.release),
            checked: Arc::clone(&self.checked),
            ctx: self.ctx.clone(),
        }
    }

    /// Looks now, because the person asked. Nothing happens in `--demo`,
    /// while a look is under way, or once an update is being installed.
    pub fn check_now(&self) {
        if self.ctx.is_none() {
            return;
        }
        let looker = self.looker();
        thread::spawn(move || looker.look());
    }

    /// Whether the person can ask for a look.
    pub fn can_check(&self) -> bool {
        self.ctx.is_some()
            && *self.checked.lock().expect("update") != Checked::Checking
            && matches!(
                *self.phase.lock().expect("update"),
                Phase::Idle | Phase::Available { .. } | Phase::Failed { .. }
            )
    }

    pub fn checked(&self) -> Checked {
        self.checked.lock().expect("update").clone()
    }

    pub fn offer(&self) -> Option<Offer> {
        let (version, downloading, failed) = match self.phase.lock().expect("update").clone() {
            Phase::Available { version } => (version, false, false),
            Phase::Downloading { version } => (version, true, false),
            Phase::Failed { version } => (version, false, true),
            Phase::Idle | Phase::Installing => return None,
        };
        Some(Offer {
            version,
            downloading,
            failed,
        })
    }

    /// The helper has the update: the app should quit so it can install.
    pub fn installing(&self) -> bool {
        *self.phase.lock().expect("update") == Phase::Installing
    }

    /// Downloads, verifies and hands the update to the helper, off the UI thread.
    pub fn install(&self) {
        let Some(release) = self.release.lock().expect("update").clone() else {
            return;
        };
        {
            let mut phase = self.phase.lock().expect("update");
            if !matches!(*phase, Phase::Available { .. } | Phase::Failed { .. }) {
                return;
            }
            *phase = Phase::Downloading {
                version: release.version.clone(),
            };
        }
        log::info!("updates: downloading {}", release.version);
        let (phase, ctx) = (Arc::clone(&self.phase), self.ctx.clone());
        thread::spawn(move || {
            let result = updater().and_then(|updater| {
                let prepared = updater.download(&release, |_, _| {})?;
                updater.handoff(prepared, Vec::new())
            });
            *phase.lock().expect("update") = match result {
                Ok(()) => {
                    log::info!(
                        "updates: {} is verified and handed to the installer; quitting",
                        release.version
                    );
                    Phase::Installing
                }
                Err(error) => {
                    log::warn!("could not update to {}: {error:#}", release.version);
                    Phase::Failed {
                        version: release.version.clone(),
                    }
                }
            };
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }
}

/// The part of [`Updates`] a thread takes with it to look for a release.
struct Looker {
    phase: Arc<Mutex<Phase>>,
    release: Arc<Mutex<Option<Release>>>,
    checked: Arc<Mutex<Checked>>,
    ctx: Option<egui::Context>,
}

impl Looker {
    fn look(&self) {
        {
            let mut checked = self.checked.lock().expect("update");
            let busy = !matches!(
                *self.phase.lock().expect("update"),
                Phase::Idle | Phase::Available { .. } | Phase::Failed { .. }
            );
            if *checked == Checked::Checking || busy {
                return;
            }
            *checked = Checked::Checking;
        }
        self.repaint();
        let found = find();
        self.record(found);
        self.repaint();
    }

    fn record(&self, found: Found) {
        let checked = match found {
            Found::Newer(found) => {
                // A failed download of the same version stays failed, so the
                // button still says "Try again".
                let mut phase = self.phase.lock().expect("update");
                if !matches!(&*phase, Phase::Failed { version } if *version == found.version) {
                    *phase = Phase::Available {
                        version: found.version.clone(),
                    };
                }
                *self.release.lock().expect("update") = Some(found);
                Checked::Available
            }
            Found::Nothing => Checked::UpToDate,
            Found::Elsewhere { version, reason } => Checked::Elsewhere { version, reason },
            Found::Unreachable(reason) => Checked::Unreachable(reason),
        };
        *self.checked.lock().expect("update") = checked;
    }

    fn repaint(&self) {
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }
}

/// How long a finished update's staging folder is left alone: its helper
/// may still be watching that the new version started.
const STAGING_KEPT_FOR: Duration = Duration::from_secs(10 * 60);

/// Where the installation sits and updates are staged beside it: the folder
/// holding the bundle on macOS, the program's own folder elsewhere.
fn installation_folder() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"));
    match bundle {
        Some(bundle) => bundle.parent().map(Path::to_path_buf),
        None => exe.parent().map(Path::to_path_buf),
    }
}

/// Whether `name` is a staging folder of this app's updater:
/// `.claude-toolkit-update-` and sixteen lowercase hexadecimal digits.
fn is_staging_name(name: &str) -> bool {
    name.strip_prefix(".claude-toolkit-update-")
        .is_some_and(|id| {
            id.len() == 16
                && id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// Removes what finished updates left beside the installation. Each one
/// keeps the downloaded package and a copy of the previous app, tens of
/// megabytes that nothing reads again once the new version has started.
///
/// Only folders with the updater's exact name, holding its own `handoff.json`
/// and a mark that the update ended (`started` or `result.txt`), and not
/// touched for ten minutes. Returns what was removed.
pub fn clean_staging(folder: &Path, now: SystemTime) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        let is_ours = is_staging_name(&entry.file_name().to_string_lossy())
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && path.join("handoff.json").is_file()
            && (path.join("started").exists() || path.join("result.txt").exists());
        let settled = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STAGING_KEPT_FOR);
        if is_ours && settled {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {
                    log::info!(
                        "updates: removed what an earlier update left ({})",
                        path.display()
                    );
                    removed.push(path);
                }
                Err(error) => log::warn!("updates: could not remove {}: {error}", path.display()),
            }
        }
    }
    removed
}

/// Asks GitHub for the latest release, and whether this copy may replace
/// itself with it. Package-managed and development copies may not.
fn find() -> Found {
    let updater = match updater() {
        Ok(updater) => updater,
        Err(error) => {
            log::warn!("updates: {error:#}");
            return Found::Unreachable(format!("{error:#}"));
        }
    };
    let release = match updater.check() {
        Ok(Some(release)) => release,
        Ok(None) => {
            log::info!("updates: this is the newest release");
            return Found::Nothing;
        }
        Err(error) => {
            log::info!("updates: could not check ({error:#})");
            return Found::Unreachable(format!("{error:#}"));
        }
    };
    if let Err(reason) = updater.installation() {
        log::info!(
            "updates: {} is out, but this copy does not update itself ({reason})",
            release.version
        );
        return Found::Elsewhere {
            version: release.version,
            reason: reason.to_string(),
        };
    }
    log::info!("updates: {} is available", release.version);
    Found::Newer(release)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_config_is_valid() {
        CONFIG.validate().unwrap();
        assert_eq!(CONFIG.current_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn nothing_is_offered_before_a_check_finds_something() {
        let updates = Updates::disabled();
        assert_eq!(updates.offer(), None);
        assert!(!updates.installing());
    }

    #[test]
    fn each_phase_shows_as_its_offer() {
        let updates = Updates::disabled();
        *updates.phase.lock().unwrap() = Phase::Available {
            version: "0.22.0".into(),
        };
        assert_eq!(
            updates.offer(),
            Some(Offer {
                version: "0.22.0".into(),
                downloading: false,
                failed: false
            })
        );
        *updates.phase.lock().unwrap() = Phase::Failed {
            version: "0.22.0".into(),
        };
        assert!(updates.offer().unwrap().failed);
        *updates.phase.lock().unwrap() = Phase::Installing;
        assert!(updates.installing());
        assert_eq!(updates.offer(), None);
    }

    #[test]
    fn a_look_that_finds_nothing_says_so() {
        let updates = Updates::disabled();
        updates.looker().record(Found::Nothing);
        assert_eq!(updates.checked(), Checked::UpToDate);
        assert_eq!(updates.offer(), None);
    }

    #[test]
    fn a_look_that_gets_no_answer_keeps_its_reason() {
        let updates = Updates::disabled();
        updates
            .looker()
            .record(Found::Unreachable("The update server answered 404".into()));
        assert_eq!(
            updates.checked(),
            Checked::Unreachable("The update server answered 404".into())
        );
    }

    #[test]
    fn a_copy_that_cannot_replace_itself_is_told_where_it_stands() {
        let updates = Updates::disabled();
        updates.looker().record(Found::Elsewhere {
            version: "0.24.0".into(),
            reason: "Move the app to Applications, then open it to update.".into(),
        });
        assert!(
            matches!(updates.checked(), Checked::Elsewhere { version, .. } if version == "0.24.0")
        );
        assert_eq!(updates.offer(), None, "nothing to click");
    }

    #[test]
    fn the_demo_never_looks() {
        let updates = Updates::disabled();
        assert!(!updates.can_check());
        updates.check_now();
        assert_eq!(updates.checked(), Checked::Never);
    }

    fn staging(folder: &Path, name: &str, files: &[&str]) -> PathBuf {
        let path = folder.join(name);
        std::fs::create_dir_all(path.join("previous")).unwrap();
        for file in files {
            std::fs::write(path.join(file), "x").unwrap();
        }
        path
    }

    #[test]
    fn only_this_apps_finished_updates_are_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let later = SystemTime::now() + Duration::from_secs(3600);
        let finished = staging(
            dir.path(),
            ".claude-toolkit-update-0123456789abcdef",
            &["handoff.json", "started", "result.txt"],
        );
        let rolled_back = staging(
            dir.path(),
            ".claude-toolkit-update-fedcba9876543210",
            &["handoff.json", "result.txt"],
        );
        // Still under way: no mark that it ended.
        let running = staging(
            dir.path(),
            ".claude-toolkit-update-1111111111111111",
            &["handoff.json", "ready"],
        );
        // Someone else's, and things that only look alike.
        let others = [
            staging(
                dir.path(),
                ".zapfast-update-4e275de2adc186bc",
                &["handoff.json", "started"],
            ),
            staging(
                dir.path(),
                ".claude-toolkit-update-XYZ",
                &["handoff.json", "started"],
            ),
            staging(
                dir.path(),
                ".claude-toolkit-update-AAAAAAAAAAAAAAAA",
                &["handoff.json", "started"],
            ),
            staging(
                dir.path(),
                ".claude-toolkit-update-0123456789abcdef0",
                &["handoff.json", "started"],
            ),
            staging(
                dir.path(),
                ".claude-toolkit-update-2222222222222222",
                &["started"],
            ),
            staging(
                dir.path(),
                "Claude Toolkit.app",
                &["handoff.json", "started"],
            ),
        ];

        let mut removed = clean_staging(dir.path(), later);
        removed.sort();
        let mut expected = vec![finished.clone(), rolled_back.clone()];
        expected.sort();
        assert_eq!(removed, expected);
        assert!(!finished.exists() && !rolled_back.exists());
        assert!(running.exists());
        for other in others {
            assert!(other.exists(), "{}", other.display());
        }
    }

    #[test]
    fn an_update_that_just_ended_is_left_for_its_helper() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = staging(
            dir.path(),
            ".claude-toolkit-update-0123456789abcdef",
            &["handoff.json", "started"],
        );
        assert!(clean_staging(dir.path(), SystemTime::now()).is_empty());
        assert!(fresh.exists());
    }
}
