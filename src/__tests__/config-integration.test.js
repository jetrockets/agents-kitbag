import { describe, it, expect, afterAll, beforeEach, vi } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const isWindows = process.platform === 'win32';

const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-toolkit-'));
const configFile = path.join(tmpDir, 'claude_desktop_config.json');

beforeEach(() => {
  vi.resetModules();
  delete process.env.__CLAUDE_TOOLKIT_CONFIG_PATH;
});

afterAll(() => {
  delete process.env.__CLAUDE_TOOLKIT_CONFIG_PATH;
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe('config roundtrip (real filesystem)', () => {
  it('writeConfig creates file and readConfig reads it back', async () => {
    process.env.__CLAUDE_TOOLKIT_CONFIG_PATH = configFile;
    const { writeConfig, readConfig } = await import('../config-testable.js');

    const data = { mcpServers: { github: { command: 'npx' } } };
    writeConfig(data);

    expect(fs.existsSync(configFile)).toBe(true);

    const result = readConfig();
    expect(result.mcpServers.github.command).toBe('npx');
  });

  it('writeConfig creates backup on second write', async () => {
    process.env.__CLAUDE_TOOLKIT_CONFIG_PATH = configFile;
    const { writeConfig } = await import('../config-testable.js');

    writeConfig({ mcpServers: { asana: { command: 'node' } } });

    const backupDir = path.join(tmpDir, 'backups');
    expect(fs.existsSync(backupDir)).toBe(true);
    const backups = fs.readdirSync(backupDir);
    expect(backups.length).toBeGreaterThanOrEqual(1);
  });
});

describe.skipIf(!isWindows)('Windows-specific config behavior', () => {
  it('read-only lock prevents file modification', async () => {
    const lockFile = path.join(tmpDir, 'locktest.json');
    fs.writeFileSync(lockFile, '{"test":true}');

    fs.chmodSync(lockFile, 0o444);

    let writeError = null;
    try {
      fs.writeFileSync(lockFile, '{"overwritten":true}');
    } catch (e) {
      writeError = e;
    }

    expect(writeError).not.toBeNull();
    expect(writeError.code).toBe('EPERM');

    const content = JSON.parse(fs.readFileSync(lockFile, 'utf-8'));
    expect(content.test).toBe(true);

    fs.chmodSync(lockFile, 0o644);
  });

  it('resolveConfigPath returns a path with correct separators', async () => {
    const { configPath } = await import('../config.js');
    expect(configPath).not.toContain('/');
    expect(configPath).toContain('\\');
    expect(configPath).toMatch(/claude_desktop_config\.json$/);
  });

  it('resolveConfigPath points to APPDATA or MSIX-redirected path', async () => {
    const { configPath } = await import('../config.js');
    const appData = process.env.APPDATA;
    const localAppData = process.env.LOCALAPPDATA;

    const isLegacy = configPath.startsWith(appData);
    const isMsix = localAppData && configPath.includes('Packages') && configPath.includes('LocalCache');

    expect(isLegacy || isMsix).toBe(true);
  });
});

describe.skipIf(isWindows)('non-Windows config path', () => {
  it('resolveConfigPath uses Application Support on macOS/Linux', async () => {
    const { configPath } = await import('../config.js');
    expect(configPath).toContain('Application Support');
    expect(configPath).toContain('Claude');
    expect(configPath).toMatch(/claude_desktop_config\.json$/);
  });
});
