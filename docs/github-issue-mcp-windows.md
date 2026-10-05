# GitHub issue draft: Windows MCP cold-start timeout

Draft for filing at https://github.com/anthropics/claude-code/issues/new

Before posting:
- Replace `<u>` with your Windows username.
- Update the Claude Desktop version under **Environment** with the value from `Settings → About`.
- Optionally remove the **Reference implementation** line, or replace it with your own repo link.

---

## Title

```
[BUG] Windows: MCP servers spawned via `npx -y` exceed 60s init timeout due to MSIX env perturbation
```

## Body

```markdown
## Summary

On Windows, MCP servers configured with `command: "npx"` consistently fail to initialize within Claude Desktop's 60-second handshake timeout. The pattern is reproducible across multiple packages (`@modelcontextprotocol/server-github`, `@notionhq/notion-mcp-server`, `@roychri/mcp-server-asana@beta`, `figma-developer-mcp`) — none of them are the cause individually. The cause is the Windows + MSIX environment, which makes `npx`'s cold-start path exceed the timeout on every Claude restart.

## Log evidence

`%APPDATA%\Claude\logs\mcp-server-github.log` (timestamps abbreviated):

```
T+0.00s   Server started and connected successfully    ← this is cmd.exe, not the MCP server
T+0.74s   Message from client: {"method":"initialize"...}
T+60.0s   notifications/cancelled "MCP error -32001: Request timed out"
T+60.0s   Server/Client transport closed (renderer released port)
T+70.8s   GitHub MCP Server running on stdio           ← server finally ready, ~25s after Claude gave up
T+71.1s   Server transport closed
```

Every Claude Desktop restart repeats this. Cold-start of `npx -y <pkg>` on Windows is consistently 70–85s on a healthy machine; Claude waits 60s; never converges.

## Root cause: MSIX env perturbation

Five Windows-specific factors stack, of which one is dominant:

1. **MSIX `%APPDATA%` rewrite (dominant).** Claude Desktop's MSIX container rewrites `%APPDATA%` for spawned children from `C:\Users\<u>\AppData\Roaming\` to `C:\Users\<u>\AppData\Local\AnthropicClaude\app-<version>\`. npm's `_npx` cache lives under `%APPDATA%\npm-cache\_npx`, so on every Claude restart the cached extracted package is invisible and `npx -y` re-downloads the tarball from the registry. The cache *exists* on disk — just not where the spawned npm looks.
2. **`cmd.exe` wrapping.** `npx.cmd` is a batch shim, so the chain becomes `Claude → cmd.exe /C npx.cmd → node → npx-cli.js → spawn(server)`. Extra process in the path.
3. **`CreateProcess` cost.** ~50–200ms per spawn on Windows vs ~1–5ms `fork+exec` on POSIX. 4–5 nested spawns ≈ 500ms+ on Windows alone.
4. **Defender real-time scanning** during tarball extraction (thousands of small files) adds 3–5× to install time.
5. **Tagged versions** (e.g. `@beta`) always hit the registry regardless of cache state.

On macOS the same MCP configs work because: no MSIX, no `%APPDATA%` rewrite, native `execve()` (no cmd wrapper), no Defender, faster file ops. Cold `npx` on macOS finishes in 3–8s — under the 60s timeout.

## Workaround (verified working)

Don't use `npx` at runtime. Install the package globally once at setup time and write a direct `node <bin path>` to `claude_desktop_config.json`:

**Before** (fails on Windows MSIX):
```json
{
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-github"]
}
```

**After** (works; bin path resolved from `package.json`'s `bin` field after `npm install -g`):
```json
{
  "command": "C:\\Program Files\\nodejs\\node.exe",
  "args": ["C:\\Users\\<u>\\AppData\\Roaming\\npm\\node_modules\\@modelcontextprotocol\\server-github\\dist\\index.js"]
}
```

Cold start drops from ~85s to ~300ms. Claude's 60s timeout passes with two orders of magnitude headroom.

Reference implementation: <your repo link, optional>

## Suggestions

1. **Update docs/samples.** The current MCP setup guides recommend `npx -y` everywhere. On Windows MSIX this pattern is fundamentally broken. Suggest globally-installed packages + direct node path, or at least mention the limitation for Windows.
2. **Raise the 60s timeout** (or expose `MCP_TIMEOUT` env var that affects Claude Desktop runtime, not just `claude mcp list` health check as noted in #57559).
3. **Investigate the MSIX `%APPDATA%` rewrite.** It silently breaks every Node-ecosystem tool that caches under `%APPDATA%` (npm, yarn, pnpm, vite, …). If a child process is supposed to see the host's real environment, the rewrite is a packaging bug.

## Environment

- Windows 11
- Claude Desktop: <version from Settings → About>
- Node.js: 22.x LTS (system-installed)
- Reproduces with several published MCP servers, all using `npx -y` at runtime
```

---

## Related issues to cross-reference (optional)

- #57559 — local MCP servers from `~/.claude.json` not loading in CCD sessions (closed)
- #57585 — UtilityProcess spawn timeout (closed as duplicate)
- #50559 — Subprocess initialization did not complete within 60000ms on Windows
