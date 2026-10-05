//! Secrets kept outside the config file, for people without 1Password.
//!
//! Claude Desktop does not expand variables in an MCP server's `env`, so the
//! config holds a `<backend>:<name>` reference and the runner resolves it at
//! launch. Backends are whatever the OS already ships with, called by
//! absolute path: an app launched from the Dock or the Start menu only gets
//! the bare system PATH.

use std::path::PathBuf;

use crate::exec::{CommandRunner, Output};
use crate::platform::{Env, Os};
use crate::secret::Secret;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// macOS Keychain, through `/usr/bin/security`.
    Keychain,
    /// A DPAPI-encrypted file under `%APPDATA%\agents-kitbag\secrets`,
    /// decryptable only by that user on that machine.
    Dpapi,
    /// The Linux Secret Service (GNOME Keyring, KWallet), through
    /// `secret-tool`.
    Libsecret,
    /// The GitHub CLI's own token. Nothing of ours is stored.
    Gh,
}

impl Backend {
    pub fn prefix(self) -> &'static str {
        match self {
            Backend::Keychain => "keychain",
            Backend::Dpapi => "dpapi",
            Backend::Libsecret => "libsecret",
            Backend::Gh => "gh",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Backend::Keychain => "macOS Keychain",
            Backend::Dpapi => "Windows credential store (DPAPI)",
            Backend::Libsecret => "Secret Service (libsecret)",
            Backend::Gh => "GitHub CLI (gh auth token)",
        }
    }

    /// The store this operating system ships.
    pub fn system(os: Os) -> Self {
        match os {
            Os::Mac => Backend::Keychain,
            Os::Windows => Backend::Dpapi,
            Os::Linux => Backend::Libsecret,
        }
    }

    /// Whether a reference to this backend means anything on `os`: a config
    /// carried over from another system may name a store that is not here.
    pub fn readable_on(self, os: Os) -> bool {
        match self {
            Backend::Gh => true,
            other => other == Backend::system(os),
        }
    }
}

/// `<backend>:<name>`, as written into the config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretRef {
    pub backend: Backend,
    pub name: String,
}

impl SecretRef {
    pub fn parse(reference: &str) -> Result<Self, String> {
        let Some((prefix, name)) = reference.split_once(':') else {
            return Err(format!(
                "Secret reference needs a backend prefix, got \"{reference}\""
            ));
        };
        let backend = [
            Backend::Keychain,
            Backend::Dpapi,
            Backend::Libsecret,
            Backend::Gh,
        ]
        .into_iter()
        .find(|b| b.prefix() == prefix)
        .ok_or_else(|| format!("Unknown backend \"{prefix}\""))?;
        Ok(Self {
            backend,
            name: name.to_owned(),
        })
    }

    /// The reference a server's token is kept under.
    pub fn for_key(backend: Backend, config_key: &str) -> Self {
        Self {
            backend,
            // The GitHub CLI is the store; there is nothing of ours to name.
            name: match backend {
                Backend::Gh => String::new(),
                _ => format!("agents-kitbag-{config_key}"),
            },
        }
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.backend.prefix(), self.name)
    }
}

/// Tests name a `gh` of their own (or one that is not there), whatever is
/// installed.
pub const GH_PATH_OVERRIDE: &str = "__AGENTS_KITBAG_GH_PATH";

/// The backend's program by absolute path, or its bare name for an install
/// in a place this list does not know.
pub fn resolve_bin(env: &Env, name: &str) -> String {
    if name == "gh"
        && let Some(path) = env.var(GH_PATH_OVERRIDE)
    {
        return path.to_owned();
    }
    let candidates: Vec<PathBuf> = match (name, env.os) {
        ("gh", Os::Mac) => ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        ("gh", Os::Linux) => [
            "/usr/bin/gh",
            "/usr/local/bin/gh",
            "/snap/bin/gh",
            "/home/linuxbrew/.linuxbrew/bin/gh",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        ("secret-tool", _) => ["/usr/bin/secret-tool", "/usr/local/bin/secret-tool"]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        ("gh", Os::Windows) => [
            env.path_under("ProgramFiles", &["GitHub CLI", "gh.exe"]),
            env.path_under("LOCALAPPDATA", &["Microsoft", "WinGet", "Links", "gh.exe"]),
            env.path_under("LOCALAPPDATA", &["Programs", "GitHub CLI", "gh.exe"]),
        ]
        .into_iter()
        .flatten()
        .collect(),
        ("security", _) => vec![PathBuf::from("/usr/bin/security")],
        ("powershell", _) => env
            .path_under(
                "SystemRoot",
                &["System32", "WindowsPowerShell", "v1.0", "powershell.exe"],
            )
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    Env::first_existing(candidates)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_owned())
}

/// A DPAPI entry's name becomes part of a PowerShell script and of a file
/// name, so only the characters Agents Kitbag itself writes are accepted.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Windows PowerShell 5.1 started from a process that PowerShell 7 started
/// (a pwsh terminal, a CI step, an app launched from either) inherits a
/// module path that names PowerShell 7's modules, and then cannot load its
/// own: "The 'ConvertTo-SecureString' command was found in the module
/// 'Microsoft.PowerShell.Security', but the module could not be loaded".
/// Every script starts by putting its own modules back.
const PS_OWN_MODULES: &str = r"$env:PSModulePath = (Join-Path $PSHOME 'Modules') + ';' + [Environment]::GetEnvironmentVariable('PSModulePath', 'Machine')";

// DPAPI encrypts to the current user on the current machine, so the
// ciphertext file is useless anywhere else. The plaintext travels through
// stdin, never as a command argument.
fn ps_read(name: &str) -> String {
    format!(
        r"
{PS_OWN_MODULES}
$ErrorActionPreference = 'Stop'
$file = Join-Path $env:APPDATA 'agents-kitbag\secrets\{name}.dpapi'
if (-not (Test-Path $file)) {{ exit 1 }}
$secure = Get-Content $file | ConvertTo-SecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try {{ [Console]::Out.Write([Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)) }}
finally {{ [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }}
"
    )
}

fn ps_write(name: &str) -> String {
    format!(
        r"
{PS_OWN_MODULES}
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:APPDATA 'agents-kitbag\secrets'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$plain = [Console]::In.ReadToEnd()
$secure = ConvertTo-SecureString $plain -AsPlainText -Force
ConvertFrom-SecureString $secure | Set-Content -Path (Join-Path $dir '{name}.dpapi') -NoNewline
"
    )
}

const POWERSHELL: [&str; 3] = ["-NoProfile", "-NonInteractive", "-Command"];

/// The operating system's credential stores.
pub struct Stores<'a> {
    pub env: &'a Env,
    pub runner: &'a dyn CommandRunner,
}

impl Stores<'_> {
    fn bin(&self, name: &str) -> String {
        resolve_bin(self.env, name)
    }

    fn user(&self) -> &str {
        self.env.var("USER").unwrap_or_default()
    }

    fn powershell(&self, script: &str, input: Option<&str>) -> Output {
        let mut args = POWERSHELL.to_vec();
        args.push(script);
        self.runner.run(&self.bin("powershell"), &args, input)
    }

    /// The secret, or `None` when the entry is missing or the store is locked.
    pub fn read(&self, reference: &SecretRef) -> Option<Secret> {
        let name = reference.name.as_str();
        let output = match reference.backend {
            Backend::Keychain => self.runner.run(
                &self.bin("security"),
                &["find-generic-password", "-a", self.user(), "-s", name, "-w"],
                None,
            ),
            Backend::Dpapi if safe_name(name) => self.powershell(&ps_read(name), None),
            Backend::Gh => self.runner.run(&self.bin("gh"), &["auth", "token"], None),
            Backend::Libsecret => {
                self.runner
                    .run(&self.bin("secret-tool"), &["lookup", "service", name], None)
            }
            Backend::Dpapi => return None,
        };
        let value = output.stdout.trim_end_matches(['\r', '\n']);
        (output.ok() && !value.is_empty()).then(|| Secret::new(value))
    }

    /// Stores the secret, or fails with the backend's own message.
    pub fn store(&self, reference: &SecretRef, secret: &Secret) -> Result<(), String> {
        let name = reference.name.as_str();
        let value = secret.expose();
        let output = match reference.backend {
            // gh owns its token; there is nothing to write.
            Backend::Gh => return Ok(()),
            // `-w` last makes security prompt for the value and its
            // confirmation, so both come from stdin and the secret stays out
            // of the arguments.
            Backend::Keychain => self.runner.run(
                &self.bin("security"),
                &[
                    "add-generic-password",
                    "-a",
                    self.user(),
                    "-s",
                    name,
                    "-U",
                    "-w",
                ],
                Some(&format!("{value}\n{value}\n")),
            ),
            Backend::Dpapi if safe_name(name) => self.powershell(&ps_write(name), Some(value)),
            Backend::Dpapi => return Err(format!("\"{name}\" is not a name a secret can have")),
            // The secret is read from stdin when it is not a terminal.
            Backend::Libsecret => self.runner.run(
                &self.bin("secret-tool"),
                &["store", "--label", name, "service", name],
                Some(value),
            ),
        };
        if output.ok() {
            return Ok(());
        }
        let message = output.stderr.trim();
        Err(if message.is_empty() {
            format!(
                "{} failed with code {}",
                reference.backend.label(),
                output.code
            )
        } else {
            message.to_owned()
        })
    }

    /// Removes the entry. Nothing happens when it is not there, or for the
    /// GitHub CLI, whose token is its own.
    pub fn remove(&self, reference: &SecretRef) {
        let name = reference.name.as_str();
        match reference.backend {
            Backend::Gh => {}
            Backend::Keychain => {
                self.runner.run(
                    &self.bin("security"),
                    &["delete-generic-password", "-a", self.user(), "-s", name],
                    None,
                );
            }
            Backend::Libsecret => {
                self.runner
                    .run(&self.bin("secret-tool"), &["clear", "service", name], None);
            }
            Backend::Dpapi => {
                if safe_name(name)
                    && let Some(folder) = self
                        .env
                        .path_under("APPDATA", &["agents-kitbag", "secrets"])
                {
                    let _ = std::fs::remove_file(folder.join(format!("{name}.dpapi")));
                }
            }
        }
    }

    /// Whether the backend's command is there and answers.
    pub fn usable(&self, backend: Backend) -> bool {
        match backend {
            // Whether there is a login keychain to write to. Not
            // `security error 0`, the probe the Node.js toolkit used: on macOS 15 it
            // prints "No error." and exits 1, so the Keychain was never
            // offered.
            Backend::Keychain => self
                .runner
                .run(
                    &self.bin("security"),
                    &["default-keychain", "-d", "user"],
                    None,
                )
                .ok(),
            // That the cmdlets the store is built on can be loaded, not
            // only that PowerShell starts.
            Backend::Dpapi => self
                .powershell(
                    &format!(
                        "{PS_OWN_MODULES}\n$ErrorActionPreference = 'Stop'\nGet-Command ConvertTo-SecureString, ConvertFrom-SecureString | Out-Null"
                    ),
                    None,
                )
                .ok(),
            Backend::Gh => self
                .runner
                .run(&self.bin("gh"), &["auth", "token"], None)
                .ok(),
            // A lookup of an entry that is not there: exit 1 with a Secret
            // Service to ask, an error (or no program) without one. Not
            // `--version`, which older releases do not have.
            Backend::Libsecret => {
                let probe = self.runner.run(
                    &self.bin("secret-tool"),
                    &["lookup", "service", "agents-kitbag-probe"],
                    None,
                );
                matches!(probe.code, 0 | 1) && probe.stderr.trim().is_empty()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRunner, fail, ok};

    fn mac() -> Env {
        Env::with(Os::Mac, "/Users/me", &[("USER", "me")])
    }

    #[test]
    fn the_github_cli_can_be_named_from_outside() {
        let env = Env::with(Os::Mac, "/h", &[(GH_PATH_OVERRIDE, "/nowhere/gh")]);
        assert_eq!(resolve_bin(&env, "gh"), "/nowhere/gh");
        // Only gh: every other program is still looked up as before.
        assert_ne!(resolve_bin(&env, "powershell"), "/nowhere/gh");
    }

    #[test]
    fn references_round_trip() {
        for text in [
            "keychain:agents-kitbag-asana",
            "dpapi:agents-kitbag-jira-acme",
            "gh:",
        ] {
            assert_eq!(SecretRef::parse(text).unwrap().to_string(), text);
        }
        assert!(SecretRef::parse("no-prefix").is_err());
        assert!(SecretRef::parse("vault:thing").is_err());
        // A URL is not a reference: its "backend" is unknown.
        assert!(SecretRef::parse("https://example.com").is_err());
    }

    #[test]
    fn a_servers_reference_is_named_after_its_config_key() {
        assert_eq!(
            SecretRef::for_key(Backend::Keychain, "jira-acme").to_string(),
            "keychain:agents-kitbag-jira-acme"
        );
        assert_eq!(SecretRef::for_key(Backend::Gh, "github").to_string(), "gh:");
    }

    #[test]
    fn the_keychain_is_asked_for_this_users_entry() {
        let runner = FakeRunner::default().on("security", "find-generic-password", ok("tok\n"));
        let env = mac();
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        let secret = stores.read(&SecretRef::parse("keychain:agents-kitbag-asana").unwrap());
        assert_eq!(secret.unwrap().expose(), "tok");
        assert_eq!(
            runner.calls()[0].args,
            [
                "find-generic-password",
                "-a",
                "me",
                "-s",
                "agents-kitbag-asana",
                "-w"
            ]
        );
    }

    #[test]
    fn a_missing_entry_reads_as_none() {
        let runner =
            FakeRunner::default().on("security", "find-generic-password", fail(44, "nope"));
        let env = mac();
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        assert!(
            stores
                .read(&SecretRef::parse("keychain:x").unwrap())
                .is_none()
        );
        assert!(
            stores
                .read(&SecretRef::parse("libsecret:x").unwrap())
                .is_none()
        );
    }

    #[test]
    fn a_secret_is_stored_through_stdin_never_the_command_line() {
        let runner = FakeRunner::default().on("security", "add-generic-password", ok(""));
        let env = mac();
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        stores
            .store(
                &SecretRef::for_key(Backend::Keychain, "asana"),
                &Secret::new("s3cret"),
            )
            .unwrap();
        assert!(!runner.all_arguments().contains("s3cret"));
        assert_eq!(runner.calls()[0].input.as_deref(), Some("s3cret\ns3cret\n"));
        assert_eq!(runner.calls()[0].args.last().unwrap(), "-w");
    }

    #[test]
    fn a_failed_store_says_what_the_backend_said() {
        let runner =
            FakeRunner::default().on("security", "add-generic-password", fail(1, "locked\n"));
        let env = mac();
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        let error = stores
            .store(
                &SecretRef::for_key(Backend::Keychain, "asana"),
                &Secret::new("x"),
            )
            .unwrap_err();
        assert_eq!(error, "locked");
    }

    #[test]
    fn dpapi_goes_through_powershell_with_the_secret_on_stdin() {
        let runner = FakeRunner::default().on("powershell", "ReadToEnd", ok(""));
        let env = Env::with(Os::Windows, "C:/Users/me", &[]);
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        stores
            .store(
                &SecretRef::for_key(Backend::Dpapi, "asana"),
                &Secret::new("s3cret"),
            )
            .unwrap();
        let call = &runner.calls()[0];
        assert_eq!(call.input.as_deref(), Some("s3cret"));
        assert!(call.args[3].contains("agents-kitbag-asana.dpapi"));
        assert!(!runner.all_arguments().contains("s3cret"));
    }

    #[test]
    fn a_dpapi_name_cannot_carry_a_script() {
        let runner = FakeRunner::default().on("powershell", "", ok("x"));
        let env = Env::with(Os::Windows, "C:/Users/me", &[]);
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        let evil = SecretRef {
            backend: Backend::Dpapi,
            name: "x'; Remove-Item C:\\ -Recurse; '".to_owned(),
        };
        assert!(stores.read(&evil).is_none());
        assert!(stores.store(&evil, &Secret::new("x")).is_err());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn linux_keeps_secrets_in_the_secret_service() {
        let runner = FakeRunner::default()
            .on("secret-tool", "store", ok(""))
            .on("secret-tool", "agents-kitbag-asana", ok("tok\n"))
            .on("secret-tool", "agents-kitbag-probe", fail(1, ""));
        let env = Env::with(Os::Linux, "/home/me", &[]);
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        assert_eq!(Backend::system(Os::Linux), Backend::Libsecret);
        assert!(stores.usable(Backend::Libsecret));
        let reference = SecretRef::for_key(Backend::Libsecret, "asana");
        assert_eq!(reference.to_string(), "libsecret:agents-kitbag-asana");
        stores.store(&reference, &Secret::new("s3cret")).unwrap();
        let call = &runner.calls()[1];
        assert_eq!(
            call.args,
            [
                "store",
                "--label",
                "agents-kitbag-asana",
                "service",
                "agents-kitbag-asana"
            ]
        );
        assert_eq!(call.input.as_deref(), Some("s3cret"));
        assert!(!runner.all_arguments().contains("s3cret"));
        assert_eq!(stores.read(&reference).unwrap().expose(), "tok");
    }

    #[test]
    fn a_secret_service_that_does_not_answer_is_not_usable() {
        let env = Env::with(Os::Linux, "/home/me", &[]);
        let no_daemon = FakeRunner::default().on(
            "secret-tool",
            "agents-kitbag-probe",
            fail(
                1,
                "secret-tool: Cannot autolaunch D-Bus without X11 $DISPLAY",
            ),
        );
        assert!(
            !Stores {
                env: &env,
                runner: &no_daemon
            }
            .usable(Backend::Libsecret)
        );
        let not_installed = FakeRunner::default();
        assert!(
            !Stores {
                env: &env,
                runner: &not_installed
            }
            .usable(Backend::Libsecret)
        );
    }

    #[test]
    fn a_store_from_another_system_is_not_readable_here() {
        assert!(Backend::Keychain.readable_on(Os::Mac));
        assert!(!Backend::Keychain.readable_on(Os::Linux));
        assert!(Backend::Libsecret.readable_on(Os::Linux));
        assert!(!Backend::Libsecret.readable_on(Os::Mac));
        assert!(!Backend::Dpapi.readable_on(Os::Mac));
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            assert!(Backend::Gh.readable_on(os));
        }
    }

    #[test]
    fn gh_is_the_store_so_storing_writes_nothing() {
        let runner = FakeRunner::default().on("gh", "token", ok("gho_abc\n"));
        let env = mac();
        let stores = Stores {
            env: &env,
            runner: &runner,
        };
        let reference = SecretRef::for_key(Backend::Gh, "github");
        stores.store(&reference, &Secret::new("ignored")).unwrap();
        assert!(runner.calls().is_empty());
        assert_eq!(stores.read(&reference).unwrap().expose(), "gho_abc");
        assert!(stores.usable(Backend::Gh));
        assert!(!stores.usable(Backend::Libsecret));
    }

    /// This system's own store, for real: the Keychain through `security`,
    /// DPAPI through PowerShell, the Secret Service through `secret-tool`.
    /// Stored from stdin, read back, replaced, removed; nothing is left.
    ///
    /// Ignored: it writes to the login store. CI runs it on macOS and
    /// Windows; on Linux it needs a session bus and an unlocked keyring.
    ///
    ///     cargo test -p agents-kitbag-core --features net --lib store::tests::native_store_round_trip -- --ignored --exact
    #[test]
    #[ignore = "writes to the real credential store"]
    fn native_store_round_trip() {
        use crate::exec::SystemRunner;
        let env = Env::current();
        let stores = Stores {
            env: &env,
            runner: &SystemRunner,
        };
        let backend = Backend::system(env.os);
        assert!(stores.usable(backend), "{} answers", backend.label());
        let reference = SecretRef {
            backend,
            name: format!("claude-agents-kitbag-core-test-{}", std::process::id()),
        };

        assert!(stores.read(&reference).is_none(), "nothing there yet");
        let stored = stores.store(&reference, &Secret::new("first value"));
        let first = stores.read(&reference);
        // On Windows the entry is a file, and it must not be plain text.
        let on_disk = env
            .path_under("APPDATA", &["agents-kitbag", "secrets"])
            .filter(|_| backend == Backend::Dpapi)
            .map(|folder| folder.join(format!("{}.dpapi", reference.name)))
            .map(|file| std::fs::read_to_string(file).unwrap_or_default());
        let replaced = stores.store(&reference, &Secret::new("p@ss w0rd/+=$'x"));
        let second = stores.read(&reference);
        stores.remove(&reference);

        assert_eq!(stored, Ok(()));
        assert_eq!(first.unwrap().expose(), "first value");
        if let Some(text) = on_disk {
            assert!(
                !text.is_empty() && !text.contains("first value"),
                "encrypted on disk"
            );
        }
        assert_eq!(replaced, Ok(()));
        assert_eq!(second.unwrap().expose(), "p@ss w0rd/+=$'x");
        assert!(stores.read(&reference).is_none(), "the entry is gone");
    }
}
