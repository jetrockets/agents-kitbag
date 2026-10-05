//! One window at a time.
//!
//! Two copies running would each write the config and the log, and each
//! could start an update of the same installation. A second start hands over
//! to the first instead: it asks the running window to come forward, and
//! exits.
//!
//! The first copy holds a lock on a file for as long as it runs; the system
//! releases it when the process ends, however it ends, so a crash never
//! leaves the app locked out. The second copy leaves a note beside the
//! lock, which the first one watches for.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

const LOCK: &str = "instance.lock";
const NOTE: &str = "instance.show";

/// How often the running copy looks for a note from a second start.
const WATCH_EVERY: Duration = Duration::from_millis(400);

/// The running copy's hold on being the only one.
pub struct Instance {
    /// Held, never read: dropping it lets another copy start.
    _lock: File,
    note: PathBuf,
}

pub enum Claim {
    /// This is the only copy running.
    First(Instance),
    /// Another copy is running, and has been asked to show its window.
    Second,
    /// The lock could not be tried (no folder to keep it in): the app starts
    /// without the guard rather than not at all.
    Unguarded,
}

/// Tries to become the one running copy, keeping the lock in `folder`.
pub fn claim(folder: &Path) -> Claim {
    if let Err(error) = std::fs::create_dir_all(folder) {
        log::warn!("no single-instance guard: {error}");
        return Claim::Unguarded;
    }
    let lock = match File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(folder.join(LOCK))
    {
        Ok(lock) => lock,
        Err(error) => {
            log::warn!("no single-instance guard: {error}");
            return Claim::Unguarded;
        }
    };
    let note = folder.join(NOTE);
    match lock.try_lock() {
        Ok(()) => {
            // A note left while nothing was running means nothing now.
            let _ = std::fs::remove_file(&note);
            Claim::First(Instance { _lock: lock, note })
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            // The process id only makes each note different from the last.
            let _ = std::fs::write(&note, std::process::id().to_string());
            Claim::Second
        }
        Err(std::fs::TryLockError::Error(error)) => {
            log::warn!("no single-instance guard: {error}");
            Claim::Unguarded
        }
    }
}

impl Instance {
    /// Whether a second start has asked for the window since the last call.
    pub fn take_request(&self) -> bool {
        self.note.exists() && std::fs::remove_file(&self.note).is_ok()
    }

    /// Brings the window forward whenever a second start asks, for as long
    /// as the app runs.
    pub fn watch(self, ctx: egui::Context) {
        let watching = std::thread::Builder::new()
            .name("instance".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(WATCH_EVERY);
                    if self.take_request() {
                        log::info!("started a second time: showing this window instead");
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    }
                }
            });
        if let Err(error) = watching {
            log::warn!("a second start will not bring this window forward: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_start_steps_aside_and_asks_for_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let Claim::First(first) = claim(dir.path()) else {
            panic!("the first start is the first");
        };
        assert!(!first.take_request(), "nobody has asked yet");

        assert!(matches!(claim(dir.path()), Claim::Second));
        assert!(first.take_request(), "the running copy hears about it");
        assert!(!first.take_request(), "once");

        assert!(matches!(claim(dir.path()), Claim::Second));
        assert!(matches!(claim(dir.path()), Claim::Second));
        assert!(first.take_request(), "several starts are one request");
    }

    #[test]
    fn the_lock_goes_with_the_copy_that_held_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = claim(dir.path());
        assert!(matches!(first, Claim::First(_)));
        assert!(matches!(claim(dir.path()), Claim::Second));
        drop(first);
        // As after a quit or a crash: the next start is the first again, and
        // the note the refused start left means nothing to it.
        let Claim::First(next) = claim(dir.path()) else {
            panic!("the lock was not released");
        };
        assert!(!next.take_request());
    }

    #[test]
    fn a_folder_that_cannot_be_made_does_not_stop_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        std::fs::write(&file, "").unwrap();
        assert!(matches!(
            claim(&file.join("under-a-file")),
            Claim::Unguarded
        ));
    }
}
