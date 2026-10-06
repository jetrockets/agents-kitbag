//! 1Password. Secrets live in a vault and the config holds only `op://`
//! references. Claude Desktop does not expand environment variables in an MCP
//! server's `env`, so a wrapper resolves the reference: the server's command
//! becomes `op run -- <original command>`, and op substitutes every `op://`
//! value in `env` before it starts the real server.

use std::io::Write;
use std::path::PathBuf;

use serde_json::json;

use crate::config::ServerConfig;
use crate::exec::CommandRunner;
use crate::platform::{Env, Os};
use crate::secret::Secret;

/// Tests and `--demo` name an `op` of their own, whatever is installed.
pub const PATH_OVERRIDE: &str = "__AGENTS_KITBAG_OP_PATH";

/// Claude Desktop launched from the Dock inherits a bare system PATH, so a
/// bare `op` in the config would not start. The standard install locations
/// are probed and an absolute path is written.
///
/// Only ever `.exe` on Windows: Claude Desktop spawns the configured command
/// directly, and a `.cmd` shim cannot be spawned without a shell
/// (CVE-2024-27980). Scoop and Chocolatey both ship an `.exe` shim beside
/// their `.cmd` one.
pub fn resolve_path(env: &Env) -> Option<PathBuf> {
    if let Some(path) = env.var(PATH_OVERRIDE) {
        return Some(PathBuf::from(path));
    }
    let candidates: Vec<PathBuf> = match env.os {
        Os::Windows => [
            env.path_under("LOCALAPPDATA", &["Microsoft", "WinGet", "Links", "op.exe"]),
            env.path_under("ProgramFiles", &["1Password CLI", "op.exe"]),
            env.path_under("ProgramFiles(x86)", &["1Password CLI", "op.exe"]),
            env.path_under("USERPROFILE", &["scoop", "shims", "op.exe"]),
            env.path_under("ProgramData", &["chocolatey", "bin", "op.exe"]),
        ]
        .into_iter()
        .flatten()
        .collect(),
        Os::Mac | Os::Linux => vec![
            PathBuf::from("/opt/homebrew/bin/op"),
            PathBuf::from("/usr/local/bin/op"),
            PathBuf::from("/usr/bin/op"),
            env.home.join(".local").join("bin").join("op"),
        ],
    };
    Env::first_existing(candidates)
}

pub fn is_secret_ref(value: &str) -> bool {
    value.starts_with("op://")
}

pub fn secret_ref(vault: &str, title: &str) -> String {
    format!("op://{vault}/{title}/credential")
}

pub fn item_title(config_key: &str) -> String {
    format!("Agents Kitbag - {config_key}")
}

/// Assignment statements land in the arguments, where any process on the
/// machine can read them off the process list. A JSON template does not.
pub fn item_template(title: &str, token: &Secret) -> String {
    json!({
        "title": title,
        "category": "API_CREDENTIAL",
        "fields": [
            { "id": "credential", "type": "CONCEALED", "label": "credential", "value": token.expose() },
            { "id": "notesPlain", "type": "STRING", "purpose": "NOTES", "label": "notesPlain", "value": "Created by agents-kitbag." },
        ],
    })
    .to_string()
}

/// Whether the server is launched through `op run`.
pub fn is_wrapped(server: &ServerConfig) -> bool {
    let bin = server
        .command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    (bin == "op" || bin == "op.exe") && server.args.first().is_some_and(|a| a == "run")
}

/// `op run` conceals secrets found on stdout by default. stdout is the MCP
/// server's JSON-RPC transport, so a token echoed back inside a response
/// would be rewritten mid-payload: hence `--no-masking`.
pub fn wrap_with_op_run(server: ServerConfig, op_binary: &str) -> ServerConfig {
    if is_wrapped(&server) {
        return server;
    }
    let mut args = vec![
        "run".to_owned(),
        "--no-masking".to_owned(),
        "--".to_owned(),
        server.command.clone(),
    ];
    args.extend(server.args.iter().cloned());
    ServerConfig {
        command: op_binary.to_owned(),
        args,
        ..server
    }
}

/// The 1Password CLI at a known path.
pub struct OnePassword<'a> {
    pub binary: String,
    pub runner: &'a dyn CommandRunner,
}

impl<'a> OnePassword<'a> {
    pub fn find(env: &Env, runner: &'a dyn CommandRunner) -> Option<Self> {
        Some(Self {
            binary: resolve_path(env)?.to_string_lossy().into_owned(),
            runner,
        })
    }

    /// The vault names, or `None` when op cannot reach them.
    ///
    /// Not `op whoami`: with the 1Password desktop app integration (rather
    /// than a session from `op signin`) whoami reports "account is not signed
    /// in" while reads succeed through biometric unlock. Listing vaults is
    /// the cheapest call that proves the CLI can reach the person's data.
    pub fn vaults(&self) -> Option<Vec<String>> {
        let output = self
            .runner
            .run(&self.binary, &["vault", "list", "--format=json"], None);
        if !output.ok() {
            return None;
        }
        let listed: serde_json::Value = serde_json::from_str(&output.stdout).ok()?;
        Some(
            listed
                .as_array()?
                .iter()
                .filter_map(|vault| vault["name"].as_str().map(str::to_owned))
                .collect(),
        )
    }

    pub fn usable(&self) -> bool {
        self.vaults().is_some()
    }

    /// Creates the item and returns its `op://` reference, or op's own
    /// message about what it objected to.
    ///
    /// `op item create -` honours a template only from a real pipe and
    /// otherwise creates an empty "Untitled" item, so the template is a file:
    /// owner-only, in a private temporary folder, removed right after. The
    /// token touches the disk for a moment instead of sitting in the
    /// arguments for every process to read.
    pub fn create_item(&self, title: &str, vault: &str, token: &Secret) -> Result<String, String> {
        let dir = tempfile::Builder::new()
            .prefix("agents-kitbag-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let template = dir.path().join("item.json");
        write_private(&template, &item_template(title, token)).map_err(|e| e.to_string())?;
        let output = self.runner.run(
            &self.binary,
            &[
                "item",
                "create",
                &format!("--template={}", template.display()),
                "--vault",
                vault,
                "--format=json",
            ],
            None,
        );
        drop(dir);
        if !output.ok() {
            let message = output.stderr.trim();
            return Err(if message.is_empty() {
                format!("op item create exited with code {}", output.code)
            } else {
                message.to_owned()
            });
        }
        // By the id op returned, not by title: saving the same server again
        // makes a second item with the same title, and op refuses to read a
        // reference that two items answer to. An id is one item for good.
        let item = serde_json::from_str::<serde_json::Value>(&output.stdout).ok();
        let named = item
            .as_ref()
            .and_then(|item| item["id"].as_str().or_else(|| item["title"].as_str()))
            .unwrap_or(title);
        Ok(secret_ref(vault, named))
    }

    pub fn read(&self, reference: &str) -> Option<Secret> {
        if !is_secret_ref(reference) {
            return None;
        }
        let output = self.runner.run(&self.binary, &["read", reference], None);
        let value = output.stdout.trim();
        (output.ok() && !value.is_empty()).then(|| Secret::new(value))
    }
}

fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, fail, ok};

    fn op(runner: &FakeRunner) -> OnePassword<'_> {
        OnePassword {
            binary: "/opt/homebrew/bin/op".to_owned(),
            runner,
        }
    }

    #[test]
    fn references_and_titles() {
        assert_eq!(
            secret_ref("Private", &item_title("asana")),
            "op://Private/Agents Kitbag - asana/credential"
        );
        assert!(is_secret_ref("op://Private/x/credential"));
        assert!(!is_secret_ref("keychain:x"));
    }

    #[test]
    fn wrapping_keeps_the_original_command_after_the_separator() {
        let server =
            ServerConfig::new("npx", ["-y", "pkg"]).with_env(&[("T", "op://v/i/credential")]);
        let wrapped = wrap_with_op_run(server.clone(), "/opt/homebrew/bin/op");
        assert_eq!(wrapped.command, "/opt/homebrew/bin/op");
        assert_eq!(
            wrapped.args,
            ["run", "--no-masking", "--", "npx", "-y", "pkg"]
        );
        assert_eq!(wrapped.env, server.env);
        assert!(is_wrapped(&wrapped));
        assert!(!is_wrapped(&server));
        // Wrapping twice changes nothing.
        assert_eq!(wrap_with_op_run(wrapped.clone(), "/elsewhere/op"), wrapped);
    }

    #[test]
    fn windows_paths_and_exe_names_count_as_op() {
        let server =
            ServerConfig::new(r"C:\Program Files\1Password CLI\OP.EXE", ["run", "--", "x"]);
        assert!(is_wrapped(&server));
        let not_run = ServerConfig::new("/usr/bin/op", ["read", "x"]);
        assert!(!is_wrapped(&not_run));
    }

    #[test]
    fn vaults_are_listed_by_name() {
        let runner = FakeRunner::default().on(
            "op",
            "vault",
            ok(r#"[{"id":"1","name":"Private"},{"id":"2","name":"Work"}]"#),
        );
        assert_eq!(op(&runner).vaults().unwrap(), ["Private", "Work"]);
    }

    #[test]
    fn a_locked_or_unintegrated_op_is_not_usable() {
        let runner = FakeRunner::default().on("op", "vault", fail(1, "not signed in"));
        assert!(!op(&runner).usable());
    }

    #[test]
    fn an_item_is_created_from_a_template_file_not_from_arguments() {
        let runner = FakeRunner::default().on(
            "op",
            "create",
            ok(r#"{"id":"abc","title":"Agents Kitbag - asana"}"#),
        );
        let reference = op(&runner)
            .create_item("Agents Kitbag - asana", "Private", &Secret::new("s3cret"))
            .unwrap();
        // The item's id: a second item with the same title cannot make
        // the reference ambiguous.
        assert_eq!(reference, "op://Private/abc/credential");
        assert!(!runner.all_arguments().contains("s3cret"));
        let call = &runner.calls()[0];
        let template = call.args[2].strip_prefix("--template=").unwrap();
        assert!(
            !std::path::Path::new(template).exists(),
            "the template is removed"
        );
        assert_eq!(&call.args[3..], ["--vault", "Private", "--format=json"]);
    }

    #[test]
    fn the_template_carries_the_token_as_a_concealed_field() {
        let template: serde_json::Value =
            serde_json::from_str(&item_template("T", &Secret::new("tok"))).unwrap();
        assert_eq!(template["category"], "API_CREDENTIAL");
        assert_eq!(template["fields"][0]["type"], "CONCEALED");
        assert_eq!(template["fields"][0]["value"], "tok");
    }

    #[test]
    fn a_refused_item_reports_ops_message() {
        let runner = FakeRunner::default().on("op", "create", fail(1, "vault is read-only\n"));
        let error = op(&runner)
            .create_item("t", "Private", &Secret::new("x"))
            .unwrap_err();
        assert_eq!(error, "vault is read-only");
    }

    #[test]
    fn only_op_references_are_read() {
        let runner = FakeRunner::default().on("op", "read", ok("tok\n"));
        assert_eq!(
            op(&runner).read("op://v/i/credential").unwrap().expose(),
            "tok"
        );
        assert!(op(&runner).read("plain-token").is_none());
    }
}
