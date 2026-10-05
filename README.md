# claude-toolkit

Cross-platform MCP setup for Claude Desktop — Jira, Linear, Notion, Azure DevOps, Asana, GitHub, Figma.

Works on **macOS** and **Windows**. Requires [Node.js](https://nodejs.org/) v20+.

> **A native app is on its way.** `crates/` holds the same toolkit as a desktop
> app for macOS, Windows and Linux (Rust, egui, [fastframe](https://github.com/crmne/fastframe)):
> every integration and the health of its token in one window, no Node.js needed
> to run it. See [Native app](#native-app-preview). The terminal menu below stays
> until the app has been verified on Windows.

## Quick start

**macOS:** Double-click `Start macOS.command`

> **First run on macOS?** Gatekeeper may block the script (`"Start macOS.command" cannot be opened because it is from an unidentified developer`). Two ways to allow it:
>
> - **Right-click** `Start macOS.command` → **Open** → **Open** in the dialog (only needed once), or
> - Open **System Settings → Privacy & Security**, scroll to the "Start macOS.command was blocked" message and click **Open Anyway**, then double-click again and confirm.
>
> If the script still fails with a permission error, make it executable: `chmod +x "Start macOS.command"`.

**Windows:** Double-click `Start Windows.bat`

**Linux:**
```bash
./start-linux.sh
```

**Or from terminal (any OS):**
```bash
npm install
node src/index.js
```

Interactive menu shows all integrations, their status, and token health:

```
   ██████╗██╗      █████╗ ██╗   ██╗██████╗ ███████╗
  ██╔════╝██║     ██╔══██╗██║   ██║██╔══██╗██╔════╝
  ██║     ██║     ███████║██║   ██║██║  ██║█████╗
  ██║     ██║     ██╔══██║██║   ██║██║  ██║██╔══╝
  ╚██████╗███████╗██║  ██║╚██████╔╝██████╔╝███████╗
   ╚═════╝╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚═════╝ ╚══════╝
              MCP Toolkit — Setup

  Token health: github ✓  jira-acme ✓  linear-nest ✓  asana ⚠ expired

? What would you like to do?
❯ Jira           ✅ (1 instance(s): jira-acme)
  Linear         ✅ (1 instance(s): linear-nest)
  Notion         ❌ (not configured)
  Azure DevOps   ❌ (not configured)
  Asana          ✅ (configured) ⚠ token expired
  GitHub         ✅ (configured)
  Figma          ❌ (not configured)
  ──────────────
  🔧 Select multiple to set up
  🔍 Check all tokens
  🗑️  Delete an integration
  🧹 Purge config backups
  🚪 Quit
```

## Features

- **1Password storage (optional)** — keep tokens in 1Password instead of the config file; see [Token storage](#token-storage)
- **Auto-discovery** — drop a new integration file into `src/integrations/` and it appears in the menu
- **Token health check** — on startup, validates all configured API tokens and flags expired or invalid ones
- **Multi-instance support** — Jira, Linear, Notion, and Azure DevOps support multiple named instances
- **Auto-restart** — after setup, Claude Desktop is restarted automatically (macOS/Windows)
- **GitHub CLI auto-install** — if `gh` is missing, offers to install it via Homebrew (macOS) or winget (Windows)

## Token storage

Claude Desktop passes an MCP server's `env` through verbatim — it does not expand
environment variables — so a token can only stay out of `claude_desktop_config.json` if
some wrapper process fetches it at launch. Setup asks where to keep each token:

| Option | Where the secret lives | Offered when |
|---|---|---|
| **GitHub CLI** | nowhere new — `gh` already has it | GitHub, and `gh auth token` works |
| **1Password** | your vault | `op` is installed and can reach your vaults |
| **System credential store** | macOS Keychain / Windows DPAPI / libsecret | always, on a supported OS |
| **Config file** | the config, in plain text | always |

The config then holds a reference rather than a secret.

### 1Password

The server is launched through `op run`, which resolves the reference at startup:

```jsonc
{
  "command": "/opt/homebrew/bin/op",
  "args": ["run", "--no-masking", "--", "npx", "-y", "@roychri/mcp-server-asana@beta"],
  "env": { "ASANA_ACCESS_TOKEN": "op://Private/Claude MCP - asana/credential" }
}
```

### System credential store

Without 1Password the token goes into whatever the OS already provides, and the server is
launched through `src/secret-runner.js`:

```jsonc
{
  "command": "/usr/local/bin/node",
  "args": ["/path/to/claude-toolkit/src/secret-runner.js",
           "--secret", "ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana",
           "--", "npx", "-y", "@roychri/mcp-server-asana@beta"],
  "env": {}
}
```

The runner exists rather than a `sh -c` one-liner because Windows has no shell to speak
of. Backends: `keychain:` (macOS `security`), `dpapi:` (Windows, DPAPI-encrypted file
under `%APPDATA%\claude-toolkit\secrets`, decryptable only by that user on that machine),
`libsecret:` (Linux `secret-tool`), and `gh:` (asks the GitHub CLI, stores nothing).

Secrets are written through stdin, never as command arguments — argv is readable by any
process on the machine. Backend commands are invoked by absolute path, since an app
launched from the Dock or Start menu only gets the bare system PATH.

### Tokens carried in headers

GitHub and Linear send their token in an `Authorization` header rather than `env`. There
the header carries a `${VAR}` placeholder, which `mcp-remote` expands from the environment
the wrapper populates — the same trick works for `op run` and for the runner.

Notes:

- **Enable the CLI integration first** — 1Password → Settings → Developer → *Integrate
  with 1Password CLI*. Without it setup falls back to plaintext. Note that with the desktop
  app integration `op whoami` reports "account is not signed in" even when everything
  works, so availability is probed by listing vaults instead.
- **Multiple 1Password accounts:** the toolkit uses whichever account `op` treats as
  default, and offers the vaults of that account only.
- **`--no-masking` is required.** `op run` conceals secrets found on stdout, and stdout is
  the MCP server's JSON-RPC transport; masking would corrupt responses that echo the token.
- **How the token gets in.** Setup hands `op` a JSON item template as a file (owner-only,
  in a private temp directory, deleted right after), never as an argv assignment — command
  arguments are readable by any process on the machine. `op item create -` is not used: it
  honours a template only from a real pipe, and Node's stdio pipes are socketpairs, which
  op silently ignores while still creating an empty "Untitled" item.
- **An absolute path to `op` is written into the config.** Claude Desktop launched from the
  Dock inherits a bare system PATH, so a bare `op` would not spawn.
- **Locked 1Password blocks server startup.** The health check reports this rather than
  claiming the token is missing. On Windows this matters more: Claude Desktop times out an
  MCP handshake after 60s, so a server whose launch waits on a Windows Hello prompt can
  lose the race. Unlock 1Password before starting Claude.
- **Windows paths.** `op.exe` is looked up in the winget shim directory, `Program Files`,
  scoop and Chocolatey shims — always the `.exe`, never a `.cmd` shim, which Node refuses
  to spawn without a shell (CVE-2024-27980). `Run-Diagnose.bat` reports which one was found
  and whether it is signed in.

Tokens already stored in plaintext stay in `backups/` until they are cleared — use
*Purge config backups* in the menu.

## Integrations

| Integration | Auth | Multi-instance | MCP Server |
|-------------|------|----------------|------------|
| **Jira** | API token (email + token) | Yes (`jira-<name>`) | [mcp-atlassian](https://github.com/sooperset/mcp-atlassian) |
| **Linear** | API key (per workspace) | Yes (`linear-<name>`) | [Official remote](https://linear.app/docs/mcp) (`mcp.linear.app`) |
| **Notion** | Personal Access Token | Yes (`notion-<name>`) | [Official](https://github.com/makenotion/notion-mcp-server) (`@notionhq/notion-mcp-server`) |
| **Azure DevOps** | Browser login (Microsoft) | Yes (`ado-<name>`) | [Official](https://github.com/microsoft/azure-devops-mcp) (`@azure-devops/mcp`) |
| **Asana** | Personal Access Token | No (one token = all workspaces) | [mcp-server-asana](https://github.com/roychri/mcp-server-asana) |
| **GitHub** | GitHub CLI (`gh auth`) | No | [Official remote](https://github.com/github/github-mcp-server) (`api.githubcopilot.com`) |
| **Figma** | Personal access token | No | [figma-developer-mcp](https://github.com/GLips/Figma-Context-MCP) |

## Skills

Claude Code skills for managers — create developer-ready issues from business requirements:

| Skill | Description |
|-------|-------------|
| [Jira Task Creator](skills/jira-task-creator-from-descriptions-as-a-project-manager/) | Analyze GitHub repos → create Jira issues |
| [Linear Task Creator](skills/linear-task-creator-from-descriptions-as-a-project-manager/) | Analyze GitHub repos → create Linear issues |
| [Asana Task Creator](skills/asana-task-creator-from-descriptions-as-a-project-manager/) | Analyze GitHub repos → create Asana tasks |
| [GitHub Task Creator](skills/github-task-creator-from-descriptions-as-a-project-manager/) | Analyze GitHub repos → create GitHub issues in Projects |
| [Azure + Linear Task Creator](skills/azure-linear-task-creator-from-descriptions-as-a-project-manager/) | Analyze Azure DevOps repos → create Linear issues |

## Project structure

```
claude-toolkit/
├── Start macOS.command        # macOS launcher (double-click)
├── Start Windows.bat          # Windows launcher (double-click)
├── start-linux.sh             # Linux launcher
├── package.json
├── src/
│   ├── index.js               # Interactive menu (auto-discovers integrations)
│   ├── config.js              # Read/write Claude Desktop config
│   ├── health-check.js        # Token validation for all integrations
│   ├── claude.js              # Claude Desktop restart helper
│   ├── backup.js              # Config backup before writes + purge
│   ├── op.js                  # 1Password CLI wrapper (secret refs, op run)
│   ├── op-storage.js          # "Where should this token be kept?" flow
│   ├── secret-store.js        # OS credential stores (Keychain/DPAPI/libsecret/gh)
│   ├── secret-runner.js       # Launches a server with secrets from those stores
│   ├── colors.js              # ANSI color constants
│   ├── network.js             # Safe fetch wrapper
│   ├── npm.js                 # npm install helpers
│   ├── platform.js            # OS detection
│   ├── validation.js          # Input validation
│   ├── multi-instance.js      # Shared multi-instance setup flow
│   ├── single-instance.js     # Shared single-instance setup flow
│   ├── __tests__/             # Vitest test suite
│   └── integrations/
│       ├── asana.js
│       ├── azure-devops.js
│       ├── figma.js
│       ├── github.js
│       ├── jira.js
│       ├── linear.js
│       └── notion.js
├── docs/                      # Internal docs and issue drafts
└── skills/
    ├── asana-task-creator-.../SKILL.md
    ├── jira-task-creator-.../SKILL.md
    ├── linear-task-creator-.../SKILL.md
    ├── github-task-creator-.../SKILL.md
    └── azure-linear-task-creator-.../SKILL.md
```

## Native app (preview)

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/screenshots/window-light.png">
  <img src="docs/screenshots/window-dark.png" alt="Claude Toolkit: the integrations in a sidebar, and a Jira server with a working token kept in the Keychain">
</picture>

```bash
cargo run -p toolkit-app -- --demo   # made-up servers, nothing sent or stored
cargo run -p toolkit-app             # your real Claude Desktop config
```

A sidebar lists the integrations with a mark for the state of their tokens; the
pane beside it shows each configured server, where its token is kept and whether
it still works, and the form that sets one up.

What it does differently from the terminal menu:

- **Claude Desktop restarts only when you click Restart.** The menu restarts it
  on its own after every setup.
- **A credential store that refuses a token is an error.** The menu falls back
  to plain text; the app shows the store's message and lets you choose again.
- **No Node.js for the toolkit.** Servers with a stored token start through
  `claude-toolkit-runner`, which sits beside the app, instead of
  `node src/secret-runner.js`. Servers the menu set up keep working, and the app
  offers to move them onto the new runner (Migrate). Tokens already in the
  Keychain or DPAPI are found under the same names.
- **On Linux the config is `~/.config/Claude/claude_desktop_config.json`**
  (`$XDG_CONFIG_HOME`), where the community builds of Claude Desktop keep it;
  the menu writes to a macOS path there. Tokens go to the Secret Service
  through `secret-tool`, under the names the menu uses.
- **A config carried over from another system** may name a store that is not
  there (a `keychain:` reference on Linux): the app says so instead of
  reporting a missing token.

The MCP servers themselves still need `npx` (Node.js) or `uvx`.

Packaging: `packaging/macos/bundle.sh` and `dmg.sh` make
`Claude Toolkit.app` and its disk image; `packaging/windows/package.ps1` makes
the portable archive; `packaging/linux/package.sh` makes a `tar.gz` whose
`install.sh` puts the app in `~/.local/share/claude-toolkit` and in the
applications menu. `.github/workflows/release-app.yml` builds all three on an
`app-vX.Y.Z` tag. Nothing is code-signed yet, so macOS asks on first open
(right-click the app, Open). See [AGENTS.md](AGENTS.md) for the architecture,
the checks and the release steps.

## Development

```bash
npm test            # run tests
npm run test:watch  # watch mode
```

## Adding a new integration

Create `src/integrations/foo.js`:

```js
export const meta = {
  key: 'foo',
  name: 'Foo',
  configKey: 'foo',       // single instance
  // configPrefix: 'foo-', // or multi-instance
};

export async function setup() {
  // setup logic
}
```

It auto-appears in the menu. No other files to change.

## License

[MIT](LICENSE). Copyright © 2026 JetRockets.
