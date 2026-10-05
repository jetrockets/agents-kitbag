import fs from 'node:fs';
import path from 'node:path';
import { isWindows } from './platform.js';

const PREFIX = 'claude_desktop_config_';

// A backup holds whatever the config held, tokens included. Ten generations
// meant a token stayed readable on disk long after it was rotated out of the
// live config, so we keep just enough to recover from a bad write.
export const RETENTION = 3;

// On Windows the live config is kept read-only so Claude cannot clobber
// mcpServers on restart (see config.js). A copy taken while that flag was set
// inherits it, and unlink then fails with EPERM — clear it and retry.
function removeFile(filePath) {
  try {
    fs.unlinkSync(filePath);
  } catch (e) {
    if (!isWindows || e.code !== 'EPERM') throw e;
    fs.chmodSync(filePath, 0o666);
    fs.unlinkSync(filePath);
  }
}

function backupDirFor(configPath) {
  return path.join(path.dirname(configPath), 'backups');
}

function listBackups(backupDir) {
  return fs.readdirSync(backupDir)
    .filter(f => f.startsWith(PREFIX))
    .sort()
    .reverse();
}

export function backupConfig(configPath) {
  if (!fs.existsSync(configPath)) return;

  const backupDir = backupDirFor(configPath);
  fs.mkdirSync(backupDir, { recursive: true });

  const timestamp = new Date().toISOString().replace(/[-:T]/g, '').slice(0, 15);
  const backupPath = path.join(backupDir, `${PREFIX}${timestamp}.json`);
  fs.copyFileSync(configPath, backupPath);

  for (const old of listBackups(backupDir).slice(RETENTION)) {
    removeFile(path.join(backupDir, old));
  }
}

// Returns how many backups were removed.
export function purgeBackups(configPath) {
  const backupDir = backupDirFor(configPath);
  if (!fs.existsSync(backupDir)) return 0;

  const backups = listBackups(backupDir);
  for (const file of backups) {
    removeFile(path.join(backupDir, file));
  }
  return backups.length;
}
