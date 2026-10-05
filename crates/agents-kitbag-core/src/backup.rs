//! A copy of the config before each write, and a way to clear them.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::platform::Os;

/// Claude Desktop's backups, named as they always were.
const DESKTOP: &str = "claude_desktop_config";
/// A folder no other program has a reason to write to: the oldest files in
/// it are deleted, and they must be this app's own.
const OWN_DIR: &str = "agents-kitbag-backups";

/// What the backups of this config are called: `<prefix><time>.<extension>`.
fn prefix(config_path: &Path) -> String {
    let stem = config_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}_", stem.trim_start_matches('.'))
}

/// A backup holds whatever the config held, tokens included. Ten generations
/// meant a token stayed readable on disk long after it was rotated out of the
/// live config, so only enough to recover from a bad write is kept.
pub const RETENTION: usize = 3;

/// Claude Desktop's config has a folder to itself, and its backups sit in
/// `backups/` beside it. Claude Code's `.claude.json` sits in the home
/// folder, so its backups go under `.claude/`; Codex's go beside its config.
fn backup_dir(config_path: &Path) -> PathBuf {
    let folder = config_path.parent().unwrap_or_else(|| Path::new("."));
    let name = config_path.file_name().unwrap_or_default();
    if config_path.file_stem().is_some_and(|stem| stem == DESKTOP) {
        folder.join("backups")
    } else if name == ".claude.json" {
        folder.join(".claude").join(OWN_DIR)
    } else {
        folder.join(OWN_DIR)
    }
}

/// Backup file names, newest first.
fn list(dir: &Path, prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(prefix))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names
}

/// On Windows the live config is kept read-only so Claude cannot drop
/// `mcpServers` on restart (see `config`). A copy taken while that flag was
/// set inherits it, and removing it then fails: clear the flag and retry.
fn remove(path: &Path, os: Os) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if os == Os::Windows && error.kind() == std::io::ErrorKind::PermissionDenied => {
            let mut permissions = std::fs::metadata(path)?.permissions();
            #[allow(
                clippy::permissions_set_readonly_false,
                reason = "Windows only, where the flag is a single attribute"
            )]
            permissions.set_readonly(false);
            std::fs::set_permissions(path, permissions)?;
            std::fs::remove_file(path)
        }
        other => other,
    }
}

/// Copies the config into `backups/` beside it and drops all but the newest
/// [`RETENTION`]. Nothing happens when there is no config yet.
pub fn backup_config(config_path: &Path, os: Os) -> std::io::Result<()> {
    if !config_path.exists() {
        return Ok(());
    }
    let dir = backup_dir(config_path);
    std::fs::create_dir_all(&dir)?;
    let prefix = prefix(config_path);
    let extension = config_path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = format!("{prefix}{}.{extension}", timestamp(SystemTime::now()));
    std::fs::copy(config_path, dir.join(name))?;
    for old in list(&dir, &prefix).into_iter().skip(RETENTION) {
        remove(&dir.join(old), os)?;
    }
    Ok(())
}

/// How many backups there are.
pub fn count(config_path: &Path) -> usize {
    list(&backup_dir(config_path), &prefix(config_path)).len()
}

/// Removes every backup; returns how many there were.
pub fn purge(config_path: &Path, os: Os) -> std::io::Result<usize> {
    let dir = backup_dir(config_path);
    let backups = list(&dir, &prefix(config_path));
    for name in &backups {
        remove(&dir.join(name), os)?;
    }
    Ok(backups.len())
}

/// `YYYYMMDDHHMMSSmmm` in UTC: names that sort by time, and two writes in
/// the same second do not share one.
fn timestamp(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since.as_secs();
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    let of_day = seconds % 86_400;
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}{:03}",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        since.subsec_millis()
    )
}

/// The date of a day counted from 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_and_sort_by_time() {
        let at = |secs: u64, millis: u64| {
            timestamp(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(millis))
        };
        assert_eq!(at(0, 0), "19700101000000000");
        // 2026-10-03T01:25:05.123Z
        assert_eq!(at(1_790_990_705, 123), "20261003012505123");
        // A leap day.
        assert_eq!(at(1_709_208_000, 0), "20240229120000000");
        assert!(at(1_790_990_705, 123) < at(1_790_990_705, 124));
    }

    #[test]
    fn nothing_is_backed_up_before_there_is_a_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude_desktop_config.json");
        backup_config(&config, Os::Mac).unwrap();
        assert!(!dir.path().join("backups").exists());
        assert_eq!(purge(&config, Os::Mac).unwrap(), 0);
    }

    #[test]
    fn only_the_newest_three_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude_desktop_config.json");
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        // Five old backups in the Node.js toolkit's naming, and a stranger's file.
        for n in 1..=5 {
            std::fs::write(
                backups.join(format!("{DESKTOP}_2026010100000{n}..json")),
                n.to_string(),
            )
            .unwrap();
        }
        std::fs::write(backups.join("notes.txt"), "mine").unwrap();
        std::fs::write(&config, "live").unwrap();

        backup_config(&config, Os::Mac).unwrap();

        let kept = list(&backups, &prefix(&config));
        assert_eq!(kept.len(), RETENTION);
        assert_eq!(
            std::fs::read_to_string(backups.join(&kept[0])).unwrap(),
            "live"
        );
        assert!(kept[1].contains("20260101000005"));
        assert!(kept[2].contains("20260101000004"));
        assert!(backups.join("notes.txt").exists());
    }

    #[test]
    fn the_other_assistants_backups_go_to_a_folder_of_the_apps_own() {
        let home = tempfile::tempdir().unwrap();
        let code = home.path().join(".claude.json");
        std::fs::write(&code, "{}").unwrap();
        backup_config(&code, Os::Mac).unwrap();
        let folder = home.path().join(".claude").join(OWN_DIR);
        let names = list(&folder, "claude_");
        assert_eq!(names.len(), 1);
        assert!(names[0].ends_with(".json"));

        let codex = home.path().join(".codex").join("config.toml");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&codex, "").unwrap();
        backup_config(&codex, Os::Mac).unwrap();
        let folder = home.path().join(".codex").join(OWN_DIR);
        let names = list(&folder, "config_");
        assert_eq!(names.len(), 1);
        assert!(names[0].ends_with(".toml"));
        assert_eq!(count(&codex), 1);
        assert_eq!(count(&code), 1);
    }

    #[test]
    fn purge_removes_every_backup_and_counts_them() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude_desktop_config.json");
        std::fs::write(&config, "one").unwrap();
        backup_config(&config, Os::Mac).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        backup_config(&config, Os::Mac).unwrap();
        assert_eq!(count(&config), 2);
        assert_eq!(purge(&config, Os::Mac).unwrap(), 2);
        assert_eq!(count(&config), 0);
        assert!(config.exists());
    }
}
