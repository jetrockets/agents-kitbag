//! Files written whole or not at all.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `bytes` beside `path` and moves the result over it, so a crash never
/// leaves half a config behind.
///
/// The file keeps what it had: a config that is a link (kept with someone's
/// dotfiles) is written through the link, and one only its owner could read
/// stays that way. A config can hold a token in plain text.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    // The file the link points at, when there is one to follow.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = target.as_path();
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(folder)?;
    let permissions = std::fs::metadata(path).ok().map(|m| m.permissions());
    let staging = staging_path(path);
    let written = (|| {
        let mut file = std::fs::File::create(&staging)?;
        // Before the bytes: the token must never sit in a file wider open
        // than the one it replaces.
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&staging, path)
    })();
    written.inspect_err(|_| {
        let _ = std::fs::remove_file(&staging);
    })
}

fn staging_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_private_file_stays_private_and_a_link_stays_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles").join("config.toml");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "one").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomic(&link, b"two").unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"two");
        let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_write_replaces_the_file_and_leaves_nothing_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["config.json"]);
    }
}
