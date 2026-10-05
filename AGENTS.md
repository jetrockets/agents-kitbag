# claude-toolkit agent guide

MCP setup for Claude Desktop. Two implementations live side by side until the
Rust one has been verified on Windows:

- `src/` is the Node.js terminal menu (`npm test`). It is the reference for
  every behaviour and message.
- `crates/` is the native app on egui/eframe and fastframe, for macOS,
  Windows and Linux.

## Rust architecture

- `toolkit-core` has no window. The config, the credential stores,
  1Password, the integrations and the health checks are plain functions over
  two traits: `exec::CommandRunner` (every external command) and `http::Http`
  (every request). Tests answer for both; nothing in a test touches the real
  Keychain, network or config. The one exception is the runner's own
  `tests/cli.rs`, which reads one real Keychain entry it creates and removes.
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

## Releasing the app

1. Bump `version` in the root `Cargo.toml`, run `cargo update -w`, commit.
   Write `packaging/release-notes/app-vX.Y.Z.md` if GitHub's generated notes
   will not do.
2. Tag `app-vX.Y.Z` and push the tag.
3. `release-app.yml` builds a universal macOS disk image, a Windows portable
   archive and a Linux `tar.gz`, writes `checksums.txt`, and publishes a
   GitHub release. Nothing is code-signed yet.

Dry run without publishing: `gh workflow run release-app.yml`.
