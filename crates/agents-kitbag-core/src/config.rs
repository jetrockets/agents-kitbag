//! An assistant's config file: where Claude Desktop's is, what servers a file
//! holds, and how to change it without losing anything else in it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::assistant::Assistant;
use crate::backup;
use crate::codex;
use crate::files;
use crate::platform::{Env, Os};

/// Tests and `--demo` point Agents Kitbag at a config of their own.
pub const PATH_OVERRIDE: &str = "__AGENTS_KITBAG_CONFIG_PATH";

const FILE_NAME: &str = "claude_desktop_config.json";

/// One entry of `mcpServers`. Keys Agents Kitbag does not know are kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub rest: Map<String, Value>,
}

impl ServerConfig {
    pub fn new(
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: None,
            rest: Map::new(),
        }
    }

    /// The same server with these variables as its whole `env`.
    pub fn with_env(mut self, vars: &[(&str, &str)]) -> Self {
        self.env = Some(
            vars.iter()
                .map(|(k, v)| ((*k).to_owned(), Value::String((*v).to_owned())))
                .collect(),
        );
        self
    }

    /// The address of a server the assistant reaches over the network
    /// rather than starts: the service's own hosted server, which signs in
    /// by itself. The app writes none of these, but finds them in configs.
    pub fn url(&self) -> Option<&str> {
        self.command
            .is_empty()
            .then(|| self.rest.get("url")?.as_str())
            .flatten()
    }

    pub fn env_var(&self, name: &str) -> Option<&str> {
        self.env.as_ref()?.get(name)?.as_str()
    }
}

/// Where Claude Desktop reads its config on this machine.
///
/// macOS: the Application Support path.
///
/// Windows has three regimes, confirmed in diagnostic reports from the field:
///
/// 1. Win 10 22H2 + MSIX Claude (older): Claude reads
///    `%APPDATA%\Claude\claude_desktop_config.json`. The MSIX-redirected
///    `LocalCache\Roaming\Claude\` folder is never created.
/// 2. Win 11 24H2 (build 26100+) + MSIX Claude (recent): Electron's
///    `userData` resolves to the package-redirected
///    `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\Roaming\Claude\`.
///    Claude reads and writes there and ignores `%APPDATA%\Claude\`; writing
///    to `%APPDATA%` is a silent no-op as far as Claude is concerned.
/// 3. Windows Server 2022 / Squirrel installs: as case 1.
///
/// Detection: if `LocalCache\Roaming\Claude\` exists under a Claude MSIX
/// package folder, Claude has launched at least once and is in case 2.
///
/// Linux: there is no official Claude Desktop. The community builds are the
/// same Electron app and keep its config under `$XDG_CONFIG_HOME/Claude`
/// (`~/.config/Claude`).
pub fn resolve_path(env: &Env) -> PathBuf {
    if let Some(path) = env.var(PATH_OVERRIDE) {
        return PathBuf::from(path);
    }
    match env.os {
        Os::Mac => env
            .home
            .join("Library")
            .join("Application Support")
            .join("Claude")
            .join(FILE_NAME),
        Os::Windows => msix_redirected(env)
            .unwrap_or_else(|| roaming(env).join("Claude"))
            .join(FILE_NAME),
        Os::Linux => env
            .var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| env.home.join(".config"))
            .join("Claude")
            .join(FILE_NAME),
    }
}

fn roaming(env: &Env) -> PathBuf {
    env.var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| env.home.join("AppData").join("Roaming"))
}

fn msix_redirected(env: &Env) -> Option<PathBuf> {
    let packages = env.path_under("LOCALAPPDATA", &["Packages"])?;
    let package = std::fs::read_dir(&packages)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("Claude_"))?;
    let redirected = package
        .path()
        .join("LocalCache")
        .join("Roaming")
        .join("Claude");
    redirected.is_dir().then_some(redirected)
}

/// The config file, read fresh for every question: Claude Desktop and the
/// person may change it at any time.
#[derive(Clone, Debug)]
pub struct ConfigFile {
    path: PathBuf,
    os: Os,
    assistant: Assistant,
}

impl ConfigFile {
    /// Claude Desktop's config at this path.
    pub fn new(path: impl Into<PathBuf>, os: Os) -> Self {
        Self::of(Assistant::ClaudeDesktop, path, os)
    }

    /// This assistant's config at this path.
    pub fn of(assistant: Assistant, path: impl Into<PathBuf>, os: Os) -> Self {
        Self {
            path: path.into(),
            os,
            assistant,
        }
    }

    pub fn locate(env: &Env) -> Self {
        Self::new(resolve_path(env), env.os)
    }

    pub fn assistant(&self) -> Assistant {
        self.assistant
    }

    fn is_toml(&self) -> bool {
        self.assistant == Assistant::Codex
    }

    /// The file as text; a missing one is empty.
    fn text(&self) -> Result<String, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(format!("The config file cannot be read: {error}")),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The whole file. A missing file is an empty config; one that cannot be
    /// read or is not a JSON object is an error, so it is never overwritten
    /// with a config that lost everything it held.
    pub fn read(&self) -> Result<Map<String, Value>, String> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
            Err(error) => return Err(format!("The config file cannot be read: {error}")),
        };
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(Map::new());
        }
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err("The config file is not a JSON object".to_owned()),
            Err(error) => Err(format!("The config file is not valid JSON: {error}")),
        }
    }

    /// Every configured server, in the file's order.
    pub fn servers(&self) -> Result<Vec<(String, ServerConfig)>, String> {
        if self.is_toml() {
            return codex::servers(&self.text()?);
        }
        Ok(servers_of(&self.read()?))
    }

    pub fn server(&self, key: &str) -> Result<Option<ServerConfig>, String> {
        Ok(self
            .servers()?
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, server)| server))
    }

    /// Adds or replaces one server and leaves every other key as it was.
    pub fn set_server(&self, key: &str, server: &ServerConfig) -> Result<(), String> {
        if self.is_toml() {
            return self.write_text(&codex::set_server(&self.text()?, key, server)?);
        }
        let mut config = self.read()?;
        let mut value = serde_json::to_value(server).map_err(|e| e.to_string())?;
        // Claude Code names the transport of every server it writes.
        if self.assistant == Assistant::ClaudeCode
            && !server.command.is_empty()
            && let Value::Object(entry) = &mut value
            && !entry.contains_key("type")
        {
            entry.insert("type".to_owned(), Value::String("stdio".to_owned()));
        }
        servers_mut(&mut config)?.insert(key.to_owned(), value);
        self.write(&config)
    }

    /// Removes these servers; returns the keys that were there.
    pub fn remove_servers(&self, keys: &[String]) -> Result<Vec<String>, String> {
        if self.is_toml() {
            let (text, removed) = codex::remove_servers(&self.text()?, keys)?;
            if !removed.is_empty() {
                self.write_text(&text)?;
            }
            return Ok(removed);
        }
        let mut config = self.read()?;
        let servers = servers_mut(&mut config)?;
        let removed: Vec<String> = keys
            .iter()
            .filter(|key| servers.shift_remove(key.as_str()).is_some())
            .cloned()
            .collect();
        if !removed.is_empty() {
            self.write(&config)?;
        }
        Ok(removed)
    }

    fn write(&self, config: &Map<String, Value>) -> Result<(), String> {
        let mut text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        text.push('\n');
        self.write_text(&text)
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.set_read_only(false);
        if let Err(error) = backup::backup_config(&self.path, self.os) {
            log::warn!("could not back up the config: {error}");
        }
        let written = files::write_atomic(&self.path, text.as_bytes())
            .map_err(|error| format!("The config file cannot be written: {error}"));
        self.set_read_only(true);
        written
    }

    /// On Windows MSIX-packaged Claude Desktop (v1.7196+), Claude rewrites
    /// the config on every restart with only a `preferences` object,
    /// stripping `mcpServers`. A read-only file blocks that while Claude can
    /// still read the servers. Verified on Win 10 22H2: with the flag custom
    /// servers load and stay; without it they are gone within seconds of a
    /// restart. On Win 11 24H2 the same holds at the redirected path. The
    /// flag is cleared before each write and set again after.
    fn set_read_only(&self, read_only: bool) {
        if self.os != Os::Windows || self.assistant != Assistant::ClaudeDesktop {
            return;
        }
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return;
        };
        let mut permissions = metadata.permissions();
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "Windows only, where the flag is a single attribute"
        )]
        permissions.set_readonly(read_only);
        if let Err(error) = std::fs::set_permissions(&self.path, permissions)
            && read_only
        {
            log::warn!(
                "could not make the config read-only ({error}); Claude may drop the servers on restart"
            );
        }
    }
}

fn servers_of(config: &Map<String, Value>) -> Vec<(String, ServerConfig)> {
    let Some(Value::Object(servers)) = config.get("mcpServers") else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(key, value)| {
            let server = serde_json::from_value(value.clone()).ok()?;
            Some((key.clone(), server))
        })
        .collect()
}

fn servers_mut(config: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, String> {
    let servers = config
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    servers
        .as_object_mut()
        .ok_or_else(|| "mcpServers in the config file is not an object".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> (tempfile::TempDir, ConfigFile) {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigFile::new(dir.path().join("Claude").join(FILE_NAME), Os::Mac);
        (dir, config)
    }

    fn asana() -> ServerConfig {
        ServerConfig::new("npx", ["-y", "@roychri/mcp-server-asana@beta"])
            .with_env(&[("ASANA_ACCESS_TOKEN", "t")])
    }

    #[test]
    fn a_missing_config_is_empty() {
        let (_dir, config) = file();
        assert!(config.read().unwrap().is_empty());
        assert!(config.servers().unwrap().is_empty());
    }

    #[test]
    fn a_server_is_written_as_the_js_toolkit_wrote_it() {
        let (_dir, config) = file();
        config.set_server("asana", &asana()).unwrap();
        assert_eq!(
            std::fs::read_to_string(config.path()).unwrap(),
            r#"{
  "mcpServers": {
    "asana": {
      "command": "npx",
      "args": [
        "-y",
        "@roychri/mcp-server-asana@beta"
      ],
      "env": {
        "ASANA_ACCESS_TOKEN": "t"
      }
    }
  }
}
"#
        );
    }

    #[test]
    fn a_server_without_env_has_no_env_key() {
        let (_dir, config) = file();
        let server = ServerConfig::new("npx", ["-y", "@azure-devops/mcp", "acme"]);
        config.set_server("ado-acme", &server).unwrap();
        let text = std::fs::read_to_string(config.path()).unwrap();
        assert!(!text.contains("env"), "{text}");
        assert_eq!(config.server("ado-acme").unwrap(), Some(server));
    }

    #[test]
    fn everything_else_in_the_file_survives_a_write() {
        let (_dir, config) = file();
        std::fs::create_dir_all(config.path().parent().unwrap()).unwrap();
        std::fs::write(
            config.path(),
            r#"{"preferences":{"z":1,"a":2},"mcpServers":{"other":{"url":"https://x","type":"http"}}}"#,
        )
        .unwrap();
        config.set_server("asana", &asana()).unwrap();
        let read = config.read().unwrap();
        assert_eq!(read["preferences"], serde_json::json!({"z": 1, "a": 2}));
        assert_eq!(
            read["mcpServers"]["other"],
            serde_json::json!({"url": "https://x", "type": "http"})
        );
        // Key order is the file's own.
        let text = std::fs::read_to_string(config.path()).unwrap();
        assert!(text.find("preferences").unwrap() < text.find("mcpServers").unwrap());
        assert!(text.find("\"z\"").unwrap() < text.find("\"a\"").unwrap());
    }

    #[test]
    fn a_config_that_does_not_parse_is_never_overwritten() {
        let (_dir, config) = file();
        std::fs::create_dir_all(config.path().parent().unwrap()).unwrap();
        std::fs::write(config.path(), "{ not json").unwrap();
        assert!(config.read().is_err());
        assert!(config.set_server("asana", &asana()).is_err());
        assert_eq!(
            std::fs::read_to_string(config.path()).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn removing_reports_only_what_was_there() {
        let (_dir, config) = file();
        config.set_server("jira-a", &asana()).unwrap();
        config.set_server("jira-b", &asana()).unwrap();
        let removed = config
            .remove_servers(&["jira-a".into(), "jira-zzz".into()])
            .unwrap();
        assert_eq!(removed, ["jira-a"]);
        let keys: Vec<_> = config
            .servers()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["jira-b"]);
    }

    #[test]
    fn claude_code_gets_its_servers_with_their_transport_named() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        std::fs::write(
            &path,
            r#"{"numStartups": 7, "mcpServers": {"mine": {"type": "http", "url": "https://x"}}, "projects": {"/work": {}}}"#,
        )
        .unwrap();
        let config = ConfigFile::of(Assistant::ClaudeCode, &path, Os::Mac);
        config.set_server("asana", &asana()).unwrap();

        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["numStartups"], 7);
        assert_eq!(written["projects"], serde_json::json!({"/work": {}}));
        assert_eq!(written["mcpServers"]["mine"]["url"], "https://x");
        assert_eq!(written["mcpServers"]["asana"]["type"], "stdio");
        assert_eq!(written["mcpServers"]["asana"]["command"], "npx");
        let keys: Vec<_> = config
            .servers()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["mine", "asana"]);
    }

    #[test]
    fn codex_keeps_its_servers_in_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# mine\nmodel = \"gpt-5\"\n").unwrap();
        let config = ConfigFile::of(Assistant::Codex, &path, Os::Windows);
        assert_eq!(config.servers().unwrap(), []);
        config.set_server("asana", &asana()).unwrap();
        assert_eq!(config.server("asana").unwrap(), Some(asana()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\nmodel = \"gpt-5\"\n"), "{text}");
        assert!(text.contains("[mcp_servers.asana]"));
        // Only Claude Desktop's config is locked against its own rewrites.
        assert!(!std::fs::metadata(&path).unwrap().permissions().readonly());
        assert_eq!(backup::count(config.path()), 1);

        assert_eq!(
            config.remove_servers(&["asana".to_owned()]).unwrap(),
            ["asana"]
        );
        assert_eq!(config.servers().unwrap(), []);

        std::fs::write(&path, "[broken").unwrap();
        assert!(config.set_server("asana", &asana()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[broken");
    }

    #[test]
    fn each_write_backs_up_what_was_there() {
        let (_dir, config) = file();
        config.set_server("asana", &asana()).unwrap();
        assert_eq!(backup::count(config.path()), 0);
        config.set_server("figma", &asana()).unwrap();
        assert_eq!(backup::count(config.path()), 1);
    }

    #[test]
    fn the_override_wins_everywhere() {
        let env = Env::with(Os::Windows, "/h", &[(PATH_OVERRIDE, "/tmp/x.json")]);
        assert_eq!(resolve_path(&env), PathBuf::from("/tmp/x.json"));
    }

    #[test]
    fn macos_reads_application_support() {
        let env = Env::with(Os::Mac, "/Users/me", &[]);
        assert_eq!(
            resolve_path(&env),
            PathBuf::from(
                "/Users/me/Library/Application Support/Claude/claude_desktop_config.json"
            )
        );
    }

    #[test]
    fn linux_reads_the_xdg_config_folder() {
        let env = Env::with(Os::Linux, "/home/me", &[]);
        assert_eq!(
            resolve_path(&env),
            PathBuf::from("/home/me/.config/Claude/claude_desktop_config.json")
        );
        let moved = Env::with(
            Os::Linux,
            "/home/me",
            &[("XDG_CONFIG_HOME", "/data/config")],
        );
        assert_eq!(
            resolve_path(&moved),
            PathBuf::from("/data/config/Claude/claude_desktop_config.json")
        );
    }

    #[test]
    fn windows_without_a_redirected_folder_uses_appdata() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("Local");
        // A Claude package that never launched: no LocalCache\Roaming\Claude.
        std::fs::create_dir_all(local.join("Packages").join("Claude_abc")).unwrap();
        let roaming = dir.path().join("Roaming");
        let env = Env::with(
            Os::Windows,
            dir.path(),
            &[
                ("APPDATA", roaming.to_str().unwrap()),
                ("LOCALAPPDATA", local.to_str().unwrap()),
            ],
        );
        assert_eq!(resolve_path(&env), roaming.join("Claude").join(FILE_NAME));
    }

    #[test]
    fn windows_with_a_redirected_folder_uses_it() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("Local");
        let redirected = local
            .join("Packages")
            .join("Claude_pzs8sxrjxfjjc")
            .join("LocalCache")
            .join("Roaming")
            .join("Claude");
        std::fs::create_dir_all(&redirected).unwrap();
        let env = Env::with(
            Os::Windows,
            dir.path(),
            &[
                ("APPDATA", dir.path().join("Roaming").to_str().unwrap()),
                ("LOCALAPPDATA", local.to_str().unwrap()),
            ],
        );
        assert_eq!(resolve_path(&env), redirected.join(FILE_NAME));
    }

    /// The real read-only attribute, which only Windows has in this form.
    #[cfg(windows)]
    #[test]
    fn on_windows_the_config_is_left_read_only_and_can_still_be_changed() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigFile::new(dir.path().join("Claude").join(FILE_NAME), Os::Windows);
        let read_only = || {
            std::fs::metadata(config.path())
                .unwrap()
                .permissions()
                .readonly()
        };

        config.set_server("asana", &asana()).unwrap();
        assert!(read_only(), "Claude cannot rewrite it on restart");
        // What Claude Desktop tries on every start.
        assert!(std::fs::write(config.path(), "{}").is_err());

        config.set_server("figma", &asana()).unwrap();
        assert!(read_only());
        assert_eq!(config.remove_servers(&["asana".into()]).unwrap(), ["asana"]);
        assert!(read_only());
        let keys: Vec<_> = config
            .servers()
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["figma"]);

        // Backups are copies of a file that was writable when copied, and
        // purging removes them all.
        assert_eq!(backup::count(config.path()), 2);
        assert_eq!(backup::purge(config.path(), Os::Windows).unwrap(), 2);
    }
}
