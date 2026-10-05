# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- GitHub issues and pull requests can be created from Claude Desktop again. The hosted server started answering write tools with a confirmation form that Claude Desktop does not draw, so nothing was written. Setup now sends `X-MCP-Features: mcp_apps_disable_form_deferral`, which makes them write directly. Set GitHub up again to pick it up

## [0.20.1] - 2026-09-10

### Fixed

- Credential backends are called by absolute path. Claude Desktop launched from the Dock inherits a bare system PATH, so a bare `gh` or `secret-tool` did not resolve and the server never started — the same trap the `op` and `npx` paths already avoid
- The runner now says which backend is unavailable instead of blaming a missing keychain entry, which sent people looking in the wrong place

## [0.20.0] - 2026-09-09

### Added

- Token storage without 1Password — setup now asks where to keep each token and can use the OS credential store: macOS Keychain, a DPAPI-encrypted file on Windows, or libsecret on Linux. The config holds a `<backend>:<name>` reference and `src/secret-runner.js` resolves it when the server starts (a wrapper rather than a `sh -c` one-liner, because Windows has no shell for it)
- GitHub can skip storage entirely — the GitHub CLI already holds the token, so the runner asks `gh auth token` at launch instead of keeping a second copy
- Secrets are written to every backend through stdin rather than argv, which any process on the machine can read

### Fixed

- `.gitattributes` pins `eol=lf`, so a Windows checkout no longer rewrites source files to CRLF

### Changed

- Health checks follow `--secret` bindings into the credential store, and report a missing entry as such — a server whose secret cannot be read never starts, so calling it an expired token would be misleading

## [0.19.0] - 2026-09-09

### Added

- Optional 1Password storage for tokens — when the 1Password CLI is installed and can reach the user's vaults, setup offers to save the token as an API Credential item and write only an `op://` reference into `claude_desktop_config.json`; the MCP server is then launched through `op run --no-masking`, which resolves the reference at startup. Declining, a missing CLI, a locked vault, or a failed item creation all fall back to the previous plaintext behaviour. The token reaches `op` as a template file (owner-only, private temp directory, deleted immediately) rather than in argv, where any process could read it
- GitHub and Linear carry their token in an `Authorization` header rather than `env`, so in 1Password mode the header gets a `${VAR}` placeholder and `mcp-remote` expands it from the environment `op run` populates
- "Purge config backups" menu entry — clears `backups/`, which mirrors the config and therefore keeps plaintext tokens alive past their rotation

### Changed

- Config backup retention cut from 10 generations to 3, for the same reason
- Windows diagnostics (`Run-Diagnose.bat`) report where `op.exe` was found and whether it can reach the user's vaults
- Health checks resolve `op://` references (and `${VAR}` placeholders) before validating, and report a locked 1Password as such instead of "token missing"

### Fixed

- Deleting a config backup on Windows no longer fails with `EPERM` — a backup copied while the config carried the read-only flag inherits it, so the flag is cleared before unlinking
- `setup.Tests.ps1` no longer errors on the npm/node mocks — `Mock npm { } -CommandName npm` passed the command name twice, leaving the script block to bind as `-MockWith`. CI never caught it because it runs `npm test` only, which does not invoke Pester

## [0.18.0] - 2026-06-08

### Added

- GitHub Projects (v2) support — the official server's `projects` toolset (`projects_list`/`projects_get`/`projects_write`) is enabled via the `X-MCP-Toolsets` header, and setup requests the `project` OAuth scope so the write tool is not filtered out

### Changed

- GitHub integration migrated from the deprecated `@modelcontextprotocol/server-github` to GitHub's official `github/github-mcp-server`, bridged via `mcp-remote` to `https://api.githubcopilot.com/mcp/` (same pattern as Linear); the token now travels in the `Authorization` header instead of the `GITHUB_PERSONAL_ACCESS_TOKEN` env var

## [0.17.1] - 2026-05-22

### Fixed

- Windows launcher menu no longer hangs at "Loading menu..." — `node src/index.js` was being run from inside a PowerShell function in `setup.ps1`, and PowerShell silently captured inquirer's entire output stream into the function's return value instead of letting it reach the console
- Windows launcher font now stays as the cmd default (Consolas) — invoking `powershell.exe` from `cmd.exe` made conhost (ForceV2) switch the window font to PowerShell's per-exe setting (Lucida Console) and never switch back
- `safeFetch` aborts hung requests after 5s (via `AbortController`) instead of blocking startup indefinitely

### Changed

- `Start Windows.bat` calls Node directly on the hot path; `setup.ps1` is invoked only when Node or `node_modules` still need to be installed
- `setup.ps1` is install-only — it no longer launches the menu
- Synced `package-lock.json` version (was left at 0.16.0)
- Added `.claude/settings.local.json` to `.gitignore`

## [0.17.0] - 2025-05-23

### Added

- Linux setup script (`start-linux.sh`) with auto-install for apt, dnf, pacman, and brew
- Bats tests for Linux and macOS launcher scripts (22 tests)
- Pester tests for Windows setup logic (14 tests)

### Changed

- Renamed `setup.bat` → `Start Windows.bat` and `setup.command` → `Start macOS.command` for clarity when file extensions are hidden
- Refactored all launcher scripts into testable functions with `main` guard
- Extracted Windows setup logic from `.bat` to `scripts/windows/setup.ps1` (`.bat` is now a thin stub)

## [0.16.0] - 2025-05-22

### Added

- `CHANGELOG.md` following Keep a Changelog format with full release history
- `/release` slash command for Claude Code with versioning rules and release checklist

### Changed

- Version scheme switched to `0.x.y` beta versioning
- `package.json` version updated to `0.16.0`

## [0.15.2] - 2025-05-22

### Changed

- Moved Windows support scripts (`Run-Diagnose.bat`, `diagnose-engine.ps1`, `install-node.ps1`) to `scripts/windows/`

## [0.15.1] - 2025-05-22

### Fixed

- Code quality improvements and Windows zip-launch guard

## [0.15.0] - 2025-05-22

### Added

- Windows MCP diagnostics tool
- GitHub Projects skill for task creators
- UTF-8 codepage support for non-English Windows locales

### Fixed

- Windows 11 24H2 config path fix
- Graceful Ctrl+C handling instead of stack trace

### Changed

- Updated README to reflect recent refactoring and new features

## [0.14.0] - 2025-05-15

### Added

- Token health check on startup

## [0.13.0] - 2025-05-15

### Changed

- Major codebase refactoring: extracted shared modules, added tests, improved structure

## [0.12.0] - 2025-05-15

### Fixed

- Replace manual `waitForEnter` prompt with auto countdown
- Ignore mouse events and stale listeners in `waitForEnter`
- Explicit cooked mode instead of `readline.createInterface`
- Restore Claude Desktop auto-restart on macOS/Linux
- Install MCP servers globally on Windows to fix MSIX cold-start timeouts

### Changed

- Notion: switch to Personal Access Tokens page

## [0.11.1] - 2025-04-15

### Added

- macOS Gatekeeper permission instructions for `setup.command`

## [0.11.0] - 2025-04-15

### Added

- Figma MCP integration
- Auto-install `gh` CLI if missing (brew/winget)
- GitHub and Azure DevOps repository support in all task creator skills

### Changed

- Updated README with Notion, Azure DevOps, banner, and integration docs

## [0.10.0] - 2025-04-13

### Added

- Azure DevOps MCP integration

## [0.9.0] - 2025-04-13

### Added

- Notion integration with multi-workspace support

## [0.8.0] - 2025-04-13

### Added

- Auto-discover installed integrations on startup
- ASCII banner

### Removed

- Manual skills installation step (now automatic)

## [0.7.0] - 2025-04-13

### Added

- Task creator skills for Asana, Jira, and Linear

### Changed

- Complete rewrite from bash to Node.js for cross-platform support (macOS, Linux, Windows)

## [0.6.0] - 2025-04-13

### Added

- Unified setup launcher — configure all integrations from one menu

## [0.5.0] - 2025-04-13

### Added

- Restricted Jira to core toolset (issues, comments, transitions, search)

### Fixed

- Improved Jira credential prompts — ask URL and email first, then API token
- Close Terminal window after pressing Enter in all setup scripts
- Strip path from Jira URL input to extract base domain

### Changed

- Jira: simplified to API token only, removed OAuth
- Linear: switched to official remote MCP server

## [0.4.0] - 2025-04-13

### Changed

- Asana: simplified to Personal Access Token only, removed OAuth

### Fixed

- Linear: updated API keys URL to correct settings path
- Linear: added workspace selection hint to token setup
- Linear: removed Bearer prefix from API key auth header
- Linear: simplified to API token only, removed OAuth

## [0.3.0] - 2025-04-13

### Added

- Linear MCP setup with OAuth, multi-instance, and task creator skill

## [0.2.0] - 2025-04-13

### Added

- Jira MCP setup with multi-instance support
- Asana MCP server setup with OAuth for Claude Desktop
- Auto-refresh Asana OAuth tokens via launchd

### Fixed

- Removed OAuth scopes not enabled in Asana app settings
- Use space-separated OAuth scopes (URL-encoded)
- Added explicit OAuth scopes for Asana granular permissions

## [0.1.0] - 2025-03-10

### Added

- Initial release
- GitHub MCP token setup script
- Asana task creator skill
- macOS bash 3.2 compatibility fix
- Claude Desktop restart handling

[unreleased]: https://github.com/jetrockets/claude-toolkit/compare/v0.17.1...HEAD
[Unreleased]: https://github.com/jetrockets/claude-toolkit/compare/v0.20.1...HEAD
[0.20.1]: https://github.com/jetrockets/claude-toolkit/compare/v0.20.0...v0.20.1
[0.20.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.19.0...v0.20.0
[0.19.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.18.0...v0.19.0
[0.18.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.17.1...v0.18.0
[0.17.1]: https://github.com/jetrockets/claude-toolkit/compare/v0.17.0...v0.17.1
[0.17.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.16.0...v0.17.0
[0.16.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.15.2...v0.16.0
[0.15.2]: https://github.com/jetrockets/claude-toolkit/compare/v0.15.1...v0.15.2
[0.15.1]: https://github.com/jetrockets/claude-toolkit/compare/v0.15.0...v0.15.1
[0.15.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.14.0...v0.15.0
[0.14.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.12.0...v0.13.0
[0.12.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/jetrockets/claude-toolkit/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jetrockets/claude-toolkit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jetrockets/claude-toolkit/releases/tag/v0.1.0
