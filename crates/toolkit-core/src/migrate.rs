//! Servers the JS toolkit set up are launched through
//! `node .../secret-runner.js`. Moving them onto the Rust runner takes Node
//! and the toolkit's own folder out of every launch.

use std::path::Path;

use crate::config::{ConfigFile, ServerConfig};
use crate::platform::Os;
use crate::runner::{self, Kind};
use crate::store::SecretRef;

/// Whether this server should be pointed at `runner_path`: it uses the Node
/// runner, or a Rust runner that is no longer where the config says.
pub fn needs_migration(server: &ServerConfig, runner_path: &str, os: Os) -> bool {
    match runner::kind(server) {
        None => false,
        Some(Kind::Native) => server.command != runner_path && !Path::new(&server.command).exists(),
        Some(Kind::Legacy) => readable_here(server, os),
    }
}

/// A server whose secrets are in another system's store stays as it is:
/// the runner could not start it here either.
fn readable_here(server: &ServerConfig, os: Os) -> bool {
    let stop = server
        .args
        .iter()
        .position(|a| a == "--")
        .unwrap_or(server.args.len());
    server.args[..stop]
        .windows(2)
        .filter(|pair| pair[0] == "--secret")
        .all(|pair| {
            pair[1]
                .split_once('=')
                .and_then(|(_, reference)| SecretRef::parse(reference).ok())
                .is_some_and(|reference| reference.backend.readable_on(os))
        })
}

/// The same server, launched by the runner at `runner_path`.
pub fn migrated(server: &ServerConfig, runner_path: &str) -> ServerConfig {
    let args = match runner::kind(server) {
        Some(Kind::Legacy) => {
            let script = server
                .args
                .iter()
                .position(|a| a.ends_with(runner::LEGACY_SCRIPT))
                .map_or(0, |at| at + 1);
            server.args[script..].to_vec()
        }
        _ => server.args.clone(),
    };
    ServerConfig {
        command: runner_path.to_owned(),
        args,
        ..server.clone()
    }
}

/// The keys of the servers [`migrate`] would rewrite.
pub fn pending(servers: &[(String, ServerConfig)], runner_path: &str, os: Os) -> Vec<String> {
    servers
        .iter()
        .filter(|(_, server)| needs_migration(server, runner_path, os))
        .map(|(key, _)| key.clone())
        .collect()
}

/// Rewrites every such server; returns their keys.
pub fn migrate(config: &ConfigFile, runner_path: &str, os: Os) -> Result<Vec<String>, String> {
    let servers = config.servers()?;
    let keys = pending(&servers, runner_path, os);
    for (key, server) in servers.iter().filter(|(key, _)| keys.contains(key)) {
        config.set_server(key, &migrated(server, runner_path))?;
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNER: &str = "/Applications/Claude Toolkit.app/Contents/MacOS/claude-toolkit-runner";

    fn legacy(reference: &str) -> ServerConfig {
        ServerConfig::new(
            "/usr/local/bin/node",
            [
                "/Users/me/claude-toolkit/src/secret-runner.js".to_owned(),
                "--secret".to_owned(),
                format!("ASANA_ACCESS_TOKEN={reference}"),
                "--".to_owned(),
                "npx".to_owned(),
                "-y".to_owned(),
                "@roychri/mcp-server-asana@beta".to_owned(),
            ],
        )
        .with_env(&[])
    }

    #[test]
    fn the_node_runner_becomes_the_rust_one_with_the_same_bindings() {
        let server = legacy("keychain:claude-mcp-asana");
        assert!(needs_migration(&server, RUNNER, Os::Mac));
        let moved = migrated(&server, RUNNER);
        assert_eq!(moved.command, RUNNER);
        assert_eq!(
            moved.args,
            [
                "--secret",
                "ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana",
                "--",
                "npx",
                "-y",
                "@roychri/mcp-server-asana@beta"
            ]
        );
        assert_eq!(moved.env, server.env);
        assert!(!needs_migration(&moved, RUNNER, Os::Mac));
    }

    #[test]
    fn a_linux_secret_is_left_on_the_node_runner() {
        let linux = legacy("libsecret:claude-mcp-asana");
        assert!(!needs_migration(&linux, RUNNER, Os::Mac));
        // On Linux itself it is an ordinary server to move.
        assert!(needs_migration(&linux, RUNNER, Os::Linux));
    }

    #[test]
    fn a_runner_that_moved_away_is_pointed_at_the_new_one() {
        let stale = ServerConfig::new(
            "/Volumes/Claude Toolkit/claude-toolkit-runner",
            ["--secret", "A=gh:", "--", "npx"],
        );
        assert!(needs_migration(&stale, RUNNER, Os::Mac));
        assert_eq!(migrated(&stale, RUNNER).args, stale.args);
        // One that is still there is not touched, wherever it is.
        let exe = std::env::current_exe().unwrap();
        let here = exe.parent().unwrap().join("claude-toolkit-runner");
        std::fs::write(&here, "").unwrap();
        let alive = ServerConfig::new(here.to_string_lossy(), ["--secret", "A=gh:", "--", "npx"]);
        assert!(!needs_migration(&alive, RUNNER, Os::Mac));
        std::fs::remove_file(here).unwrap();
    }

    #[test]
    fn plain_and_1password_servers_are_not_touched() {
        let plain = ServerConfig::new("npx", ["-y", "pkg"]);
        assert!(!needs_migration(&plain, RUNNER, Os::Mac));
        let op = ServerConfig::new("/opt/homebrew/bin/op", ["run", "--no-masking", "--", "npx"]);
        assert!(!needs_migration(&op, RUNNER, Os::Mac));
    }

    #[test]
    fn migrating_rewrites_only_what_needs_it() {
        let dir = tempfile::tempdir().unwrap();
        let config = ConfigFile::new(dir.path().join("config.json"), Os::Mac);
        config
            .set_server("asana", &legacy("keychain:claude-mcp-asana"))
            .unwrap();
        config
            .set_server(
                "figma",
                &ServerConfig::new("npx", ["-y", "figma-developer-mcp"]),
            )
            .unwrap();
        assert_eq!(migrate(&config, RUNNER, Os::Mac).unwrap(), ["asana"]);
        assert_eq!(config.server("asana").unwrap().unwrap().command, RUNNER);
        assert_eq!(config.server("figma").unwrap().unwrap().command, "npx");
        assert!(migrate(&config, RUNNER, Os::Mac).unwrap().is_empty());
    }
}
