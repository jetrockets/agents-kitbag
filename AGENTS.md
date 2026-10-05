# claude-toolkit agent guide

MCP setup for Claude Desktop: a native app on egui/eframe and fastframe, for
macOS, Windows and Linux. It replaced a Node.js terminal menu, and configs
that menu wrote are still on people's machines: the app reads them, checks
their tokens, and offers to move servers off `node .../secret-runner.js`.

## Architecture

- `toolkit-core` has no window. The config, the credential stores,
  1Password, the integrations and the health checks are plain functions over
  two traits: `exec::CommandRunner` (every external command) and `http::Http`
  (every request). Tests answer for both; nothing in a test touches the real
  credential store, network or config, except the few `#[ignore]`d ones named
  under Checks.
- An integration is a value (`integrations::Integration`): its fields, how
  its token is validated, the server config it becomes, how that token is
  checked later. A new one is one file and one line in `integrations::ALL`.
- `toolkit-runner` is the program Claude Desktop starts for a server whose
  token is in a credential store. Its stdout is the server's JSON-RPC
  transport: it prints only to stderr and never prints a secret. Its command
  line is a contract with configs already written; do not change it.
- `toolkit-app`: `ui/` draws a `Snapshot` and returns `Command`s; it never
  waits. `worker.rs` runs every command on one thread and publishes the next
  snapshot. Do not call `toolkit-core` from a view.
- Tokens travel as `Secret`, which never prints, and reach external commands
  through stdin, never through arguments. Never log a token.
- A store that refuses a token is an error shown in the form. Nothing falls
  back to plain text on its own.
- egui and winit come from the crmne forks the other fastframe apps use;
  fastframe crates share one tag. Move each group together. Explain every crate in
  `Cargo.toml`.

## Checks

    cargo fmt --all --check
    cargo clippy --locked --all-targets --all-features -- -D warnings
    RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
    cargo test --locked --all-targets --all-features

Core and the runner also type-check for Windows and Linux from a Mac:

    cargo clippy -p toolkit-core --features net -p toolkit-runner --all-targets --target x86_64-pc-windows-msvc -- -D warnings
    cargo clippy -p toolkit-core --no-default-features -p toolkit-runner --all-targets --target x86_64-unknown-linux-gnu -- -D warnings

The app does not (ring needs MSVC, OpenSSL needs its headers); CI builds and
tests it on all three.

CI (`ci.yml`) runs the format, lint and doc checks once, on Linux, and the
tests on Linux, macOS and Windows. On macOS and Windows it then runs the two
ignored tests that use the system's own credential store, the one thing
made-up commands cannot stand in for:

    cargo test -p toolkit-core --features net --lib store::tests::native_store_round_trip -- --ignored --exact
    cargo test -p toolkit-runner --test cli a_native_secret_reaches_the_server -- --ignored --exact

They write to the login Keychain or DPAPI and remove what they wrote. On
Linux they need a session bus and an unlocked keyring, so CI leaves them out
there; run them by hand.

One more test is for running by hand when setup or packages change:

    cargo test -p toolkit-core --features net --test real_setup -- --ignored --nocapture

sets Azure DevOps up (the one integration without a token), starts the
server as Claude Desktop would and waits for its answer to the MCP
`initialize`. It downloads an npm package, and on Windows installs it
globally.

`packaging.yml` builds the three packages the way the release does and
installs them, when packaging changes.

Add a focused test for every behaviour change. The window is tested
headlessly in `crates/toolkit-app/src/ui/tests.rs` (egui_kittest): real
pointer and key events, controls found by their accessible names, and the
`Command`s they send.

## Trying it

    cargo run -p toolkit-app -- --demo                          # made-up servers, nothing sent or stored
    cargo run -p toolkit-app -- --demo --select github --form
    cargo run -p toolkit-app -- --demo-shot shot.png --light --settings
    cargo run -p toolkit-app                                    # the real config

`--demo` runs in a throwaway folder with made-up answers for every command
and request, so nothing it does reaches Claude Desktop or a credential store.
The token `expired` is the one the made-up services refuse.

## Style

- Never use em dashes. Use a full stop, comma, colon, or parentheses.
- Report platform coverage honestly: say when something was only compiled.

## Releasing

A release reaches every installed copy: the app reads this repository's
latest release and offers it.

1. Bump `version` in the root `Cargo.toml`, run `cargo update -w`, commit.
   Write `packaging/release-notes/vX.Y.Z.md` if GitHub's generated notes will
   not do.
2. Tag `vX.Y.Z` (annotated) and push the tag.
3. `release.yml` builds a universal macOS disk image, a Windows portable
   archive and a Linux `tar.gz`, then waits: the publish job runs in the
   `release-signing` environment, which needs a reviewer's approval (the
   run's page, Review deployments). Once approved it signs `checksums.txt`
   with the update key and publishes a GitHub release as the latest.
   Nothing is code-signed yet.

`main` takes changes through pull requests only, with one approving review
and green `quality` and `test` checks. Release tags cannot be moved or
deleted.

A tag with a hyphen (`v0.24.0-rc1`) is a pre-release: published, never the
latest, so nobody is offered it. Dry run without publishing:
`gh workflow run release.yml`.

The names are a contract with installed copies (fastframe-update): tags are
`v<version>`, assets `claude-toolkit-v<version>-<target>` plus
`checksums.txt` and `checksums.txt.sig`, and both programs answer `--version`
with `<name> <version>`. The key is in `packaging/UPDATE_SIGNING.md`.
