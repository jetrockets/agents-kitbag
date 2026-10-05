//! The operating system and the environment, as values: path rules for
//! Windows are tested on a Mac by handing in a made-up environment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// The parts of the process environment Agents Kitbag reads.
#[derive(Clone, Debug)]
pub struct Env {
    pub os: Os,
    pub home: PathBuf,
    vars: HashMap<String, String>,
}

impl Env {
    pub fn current() -> Self {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let home = vars
            .get(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_default();
        Self {
            os: Os::current(),
            home,
            vars,
        }
    }

    /// A made-up environment, for tests and `--demo`.
    pub fn with(os: Os, home: impl Into<PathBuf>, vars: &[(&str, &str)]) -> Self {
        Self {
            os,
            home: home.into(),
            vars: vars
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    /// A variable that is set and not empty.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// `<variable>/<rest...>`, when the variable is set.
    pub fn path_under(&self, name: &str, rest: &[&str]) -> Option<PathBuf> {
        let mut path = PathBuf::from(self.var(name)?);
        path.extend(rest);
        Some(path)
    }

    /// The first candidate that exists.
    pub fn first_existing(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
        candidates.into_iter().find(|c| c.exists())
    }

    /// A program found by name on `PATH`.
    pub fn on_path(&self, program: &str) -> Option<PathBuf> {
        let names: Vec<String> = match self.os {
            Os::Windows => vec![format!("{program}.exe"), format!("{program}.cmd")],
            Os::Mac | Os::Linux => vec![program.to_owned()],
        };
        std::env::split_paths(self.var("PATH")?)
            .flat_map(|folder| names.iter().map(move |n| folder.join(n)))
            .find(|candidate| candidate.is_file())
    }
}

/// Opens a URL in the browser. Nothing waits for it.
pub fn open_url(os: Os, url: &str) {
    let result = match os {
        Os::Windows => std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn(),
        Os::Mac => std::process::Command::new("/usr/bin/open").arg(url).spawn(),
        Os::Linux => std::process::Command::new("xdg-open").arg(url).spawn(),
    };
    if let Err(error) = result {
        log::warn!("could not open {url}: {error}");
    }
}

/// Shows a folder in Finder or Explorer.
pub fn reveal(os: Os, folder: &Path) {
    let result = match os {
        Os::Windows => std::process::Command::new("explorer").arg(folder).spawn(),
        Os::Linux => std::process::Command::new("xdg-open").arg(folder).spawn(),
        Os::Mac => std::process::Command::new("/usr/bin/open")
            .arg(folder)
            .spawn(),
    };
    if let Err(error) = result {
        log::warn!("could not open {}: {error}", folder.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_variable_is_not_set() {
        let env = Env::with(Os::Windows, "/home", &[("APPDATA", ""), ("A", "b")]);
        assert_eq!(env.var("APPDATA"), None);
        assert_eq!(env.var("A"), Some("b"));
        assert_eq!(env.path_under("APPDATA", &["Claude"]), None);
        assert_eq!(
            env.path_under("A", &["x", "y"]),
            Some(PathBuf::from("b").join("x").join("y"))
        );
    }
}
