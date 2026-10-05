import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { backupConfig } from './backup.js';
import { isWindows } from './platform.js';
import { YELLOW, NC } from './colors.js';

// Resolve where Claude Desktop actually reads its config from on this OS.
//
// macOS: stable Application Support path.
//
// Windows is a maze. Three regimes confirmed in diagnostic reports from the
// field:
//
//   1. Win 10 22H2 + MSIX Claude (older): Claude reads from
//      %APPDATA%\Claude\claude_desktop_config.json directly. The
//      MSIX-redirected LocalCache\Roaming\Claude\ folder is never created.
//      → write to %APPDATA%\Claude\.
//
//   2. Win 11 24H2 (build 26100+) + MSIX Claude (recent, e.g. 1.8555): the
//      Electron `app.getPath('userData')` call now resolves to the MSIX
//      package-redirected path
//      %LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\Roaming\Claude\.
//      Claude both reads and writes there, ignoring %APPDATA%\Claude\
//      entirely. Writing to %APPDATA% on this regime is a silent no-op as
//      far as Claude is concerned. → write to the redirected path.
//
//   3. Windows Server 2022 / Squirrel installs: same as case 1.
//
// Detection: if LocalCache\Roaming\Claude\ exists under any Claude MSIX
// package directory, Claude has launched at least once and is in case 2 —
// use that path. Otherwise fall back to %APPDATA%\Claude\.
function resolveConfigPath() {
  if (process.env.__CLAUDE_TOOLKIT_CONFIG_PATH) {
    return process.env.__CLAUDE_TOOLKIT_CONFIG_PATH;
  }
  if (!isWindows) {
    return path.join(os.homedir(), 'Library', 'Application Support', 'Claude', 'claude_desktop_config.json');
  }
  const packagesDir = process.env.LOCALAPPDATA
    ? path.join(process.env.LOCALAPPDATA, 'Packages')
    : null;
  if (packagesDir) {
    try {
      const claudePkg = fs.readdirSync(packagesDir).find(n => n.startsWith('Claude_'));
      if (claudePkg) {
        const redirected = path.join(packagesDir, claudePkg, 'LocalCache', 'Roaming', 'Claude');
        if (fs.existsSync(redirected)) {
          return path.join(redirected, 'claude_desktop_config.json');
        }
      }
    } catch { /* fall through to legacy path */ }
  }
  return path.join(process.env.APPDATA, 'Claude', 'claude_desktop_config.json');
}

const configPath = resolveConfigPath();

export { configPath };

// On Windows MSIX-packaged Claude Desktop (v1.7196+), Claude internally rewrites
// claude_desktop_config.json on every restart with only a `preferences` object,
// silently stripping the user's `mcpServers` section. Marking the file read-only
// after we write blocks that clobber while still allowing Claude to *read* the
// MCP servers on startup. Empirically verified on Win 10 22H2: with read-only,
// custom MCPs (e.g. github) load and stay; without it, they're wiped within
// seconds of Claude restart. The toolkit clears the flag before each write and
// re-applies.
//
// On Win 11 24H2 this trick still applies — Claude attempts to write its
// preferences to the redirected path, hits EPERM, and gives up — but mcpServers
// stay readable. The read-only file just lives at a different path
// (LocalCache\Roaming\Claude\) chosen by resolveConfigPath() above.
function unlockConfig() {
  if (!isWindows) return;
  try { fs.chmodSync(configPath, 0o644); } catch { /* not present yet — fine */ }
}

function lockConfig() {
  if (!isWindows) return;
  try { fs.chmodSync(configPath, 0o444); } catch (e) {
    console.log(`${YELLOW}  Warning: could not lock config file read-only (${e.code || e.message}). MCP entries may be wiped on Claude restart.${NC}`);
  }
}

export function readConfig() {
  try {
    return JSON.parse(fs.readFileSync(configPath, 'utf-8'));
  } catch {
    return { mcpServers: {} };
  }
}

export function writeConfig(data) {
  unlockConfig();
  backupConfig(configPath);
  fs.mkdirSync(path.dirname(configPath), { recursive: true });
  fs.writeFileSync(configPath, JSON.stringify(data, null, 2) + '\n');
  lockConfig();
}

export function setServer(key, serverConfig) {
  const config = readConfig();
  config.mcpServers = config.mcpServers || {};
  config.mcpServers[key] = serverConfig;
  writeConfig(config);
}

export function getStatus(meta) {
  const config = readConfig();
  const servers = config.mcpServers || {};

  // Multi-instance: look for keys with prefix
  if (meta.configPrefix) {
    const instances = Object.keys(servers).filter(k => k.startsWith(meta.configPrefix));
    if (instances.length === 0) return '';
    return `${instances.length} instance(s): ${instances.join(', ')}`;
  }

  // Single instance: look for exact key
  if (meta.configKey) {
    return servers[meta.configKey] ? 'configured' : '';
  }

  return '';
}

export function deleteByMeta(meta) {
  const config = readConfig();
  const servers = config.mcpServers || {};

  let keysToDelete = [];
  if (meta.configPrefix) {
    keysToDelete = Object.keys(servers).filter(k => k.startsWith(meta.configPrefix));
  } else if (meta.configKey) {
    keysToDelete = servers[meta.configKey] ? [meta.configKey] : [];
  }

  for (const key of keysToDelete) {
    delete config.mcpServers[key];
  }

  if (keysToDelete.length > 0) writeConfig(config);
  return keysToDelete;
}
