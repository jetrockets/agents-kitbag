//! Self-update through fastframe-update.
//!
//! A newer release is offered in the sidebar and nothing is downloaded until
//! the person clicks. A check that fails (no network, no release feed yet)
//! is silent and tried again later.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

pub struct Updates {
    phase: Arc<Mutex<Phase>>,
    release: Arc<Mutex<Option<Release>>>,
    ctx: Option<egui::Context>,
}

impl Updates {
    /// No checks: `--demo`.
    pub fn disabled() -> Self {
        Self {
            phase: Arc::new(Mutex::new(Phase::Idle)),
            release: Arc::new(Mutex::new(None)),
            ctx: None,
        }
    }

    pub fn spawn(ctx: egui::Context) -> Self {
        let updates = Self {
            ctx: Some(ctx.clone()),
            ..Self::disabled()
        };
        let (phase, release) = (Arc::clone(&updates.phase), Arc::clone(&updates.release));
        thread::spawn(move || {
            loop {
                let idle = matches!(
                    *phase.lock().expect("update"),
                    Phase::Idle | Phase::Available { .. }
                );
                if idle && let Some(found) = check() {
                    *phase.lock().expect("update") = Phase::Available {
                        version: found.version.clone(),
                    };
                    *release.lock().expect("update") = Some(found);
                    ctx.request_repaint();
                }
                thread::sleep(CHECK_EVERY);
            }
        });
        updates
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
        let (phase, ctx) = (Arc::clone(&self.phase), self.ctx.clone());
        thread::spawn(move || {
            let result = updater().and_then(|updater| {
                let prepared = updater.download(&release, |_, _| {})?;
                updater.handoff(prepared, Vec::new())
            });
            *phase.lock().expect("update") = match result {
                Ok(()) => Phase::Installing,
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

/// A newer release this copy may replace itself with. Package-managed and
/// development copies may not; a failed check waits for the next one.
fn check() -> Option<Release> {
    let updater = updater()
        .inspect_err(|error| log::warn!("updates: {error:#}"))
        .ok()?;
    let release = match updater.check() {
        Ok(Some(release)) => release,
        Ok(None) => {
            log::info!("updates: this is the newest release");
            return None;
        }
        Err(error) => {
            log::info!("updates: could not check ({error:#})");
            return None;
        }
    };
    if let Err(reason) = updater.installation() {
        log::info!(
            "updates: {} is out, but this copy does not update itself ({reason})",
            release.version
        );
        return None;
    }
    log::info!("updates: {} is available", release.version);
    Some(release)
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
}
