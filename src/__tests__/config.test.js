import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

vi.mock('node:fs');
vi.mock('node:os', () => ({
  default: { homedir: () => '/mock-home' },
  homedir: () => '/mock-home',
}));

const mockConfigPath = path.join('/mock-home', 'Library', 'Application Support', 'Claude', 'claude_desktop_config.json');

beforeEach(() => {
  vi.resetModules();
  vi.restoreAllMocks();
});

describe('getStatus', () => {
  it('returns instance count for multi-instance (configPrefix)', async () => {
    fs.readFileSync = vi.fn(() => JSON.stringify({
      mcpServers: { 'jira-acme': {}, 'jira-corp': {}, 'github': {} },
    }));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { getStatus } = await import('../config.js');
    const result = getStatus({ configPrefix: 'jira-' });
    expect(result).toContain('2 instance(s)');
    expect(result).toContain('jira-acme');
    expect(result).toContain('jira-corp');
  });

  it('returns empty string when no instances match prefix', async () => {
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: { github: {} } }));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { getStatus } = await import('../config.js');
    expect(getStatus({ configPrefix: 'jira-' })).toBe('');
  });

  it('returns "configured" for single-instance (configKey)', async () => {
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: { github: { command: 'npx' } } }));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { getStatus } = await import('../config.js');
    expect(getStatus({ configKey: 'github' })).toBe('configured');
  });

  it('returns empty when configKey not found', async () => {
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: {} }));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { getStatus } = await import('../config.js');
    expect(getStatus({ configKey: 'github' })).toBe('');
  });

  it('handles missing mcpServers gracefully', async () => {
    fs.readFileSync = vi.fn(() => JSON.stringify({}));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { getStatus } = await import('../config.js');
    expect(getStatus({ configKey: 'github' })).toBe('');
  });
});

describe('configPath resolution on Windows', () => {
  // Save and restore process.platform / env per case. vi.stubGlobal doesn't
  // work for process.platform (read-only descriptor on some Node versions),
  // so we use Object.defineProperty directly.
  const originalPlatform = process.platform;
  const originalAppData = process.env.APPDATA;
  const originalLocalAppData = process.env.LOCALAPPDATA;

  const mockAppData = path.join('C:', 'Users', 'test', 'AppData', 'Roaming');
  const mockLocalAppData = path.join('C:', 'Users', 'test', 'AppData', 'Local');

  beforeEach(() => {
    Object.defineProperty(process, 'platform', { value: 'win32', configurable: true });
    process.env.APPDATA = mockAppData;
    process.env.LOCALAPPDATA = mockLocalAppData;
  });

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true });
    if (originalAppData === undefined) delete process.env.APPDATA; else process.env.APPDATA = originalAppData;
    if (originalLocalAppData === undefined) delete process.env.LOCALAPPDATA; else process.env.LOCALAPPDATA = originalLocalAppData;
  });

  it('uses MSIX-redirected path when LocalCache/Roaming/Claude exists (Win 11 24H2 regime)', async () => {
    const msixPath = path.join('C:', 'Users', 'test', 'AppData', 'Local', 'Packages', 'Claude_pzs8sxrjxfjjc', 'LocalCache', 'Roaming', 'Claude');
    fs.readdirSync = vi.fn((p) => {
      if (String(p).endsWith('Packages')) return ['Claude_pzs8sxrjxfjjc', 'OtherApp_xyz'];
      return [];
    });
    fs.existsSync = vi.fn((p) => String(p) === msixPath);
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: {} }));

    const { configPath } = await import('../config.js');
    expect(configPath).toBe(path.join(msixPath, 'claude_desktop_config.json'));
  });

  it('falls back to %APPDATA%/Claude when LocalCache/Roaming/Claude does not exist (Win 10 regime)', async () => {
    fs.readdirSync = vi.fn((p) => {
      if (String(p).endsWith('Packages')) return ['Claude_pzs8sxrjxfjjc'];
      return [];
    });
    fs.existsSync = vi.fn(() => false);
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: {} }));

    const { configPath } = await import('../config.js');
    const expected = path.join('C:', 'Users', 'test', 'AppData', 'Roaming', 'Claude', 'claude_desktop_config.json');
    expect(configPath).toBe(expected);
  });

  it('falls back to %APPDATA%/Claude when no Claude MSIX package is installed', async () => {
    fs.readdirSync = vi.fn(() => ['SomeOtherApp_xyz', 'NotClaude_abc']);
    fs.existsSync = vi.fn(() => false);
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: {} }));

    const { configPath } = await import('../config.js');
    const expected = path.join('C:', 'Users', 'test', 'AppData', 'Roaming', 'Claude', 'claude_desktop_config.json');
    expect(configPath).toBe(expected);
  });

  it('falls back to %APPDATA%/Claude when Packages directory cannot be read', async () => {
    fs.readdirSync = vi.fn(() => { throw new Error('EACCES'); });
    fs.existsSync = vi.fn(() => false);
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: {} }));

    const { configPath } = await import('../config.js');
    const expected = path.join('C:', 'Users', 'test', 'AppData', 'Roaming', 'Claude', 'claude_desktop_config.json');
    expect(configPath).toBe(expected);
  });
});

describe('deleteByMeta', () => {
  it('deletes matching prefix keys and returns them', async () => {
    const configData = { mcpServers: { 'jira-acme': {}, 'jira-corp': {}, 'github': {} } };
    fs.readFileSync = vi.fn(() => JSON.stringify(configData));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.writeFileSync = vi.fn();
    fs.chmodSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { deleteByMeta } = await import('../config.js');
    const deleted = deleteByMeta({ configPrefix: 'jira-' });
    expect(deleted).toEqual(['jira-acme', 'jira-corp']);
  });

  it('deletes single configKey', async () => {
    const configData = { mcpServers: { github: { command: 'npx' } } };
    fs.readFileSync = vi.fn(() => JSON.stringify(configData));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.writeFileSync = vi.fn();
    fs.chmodSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { deleteByMeta } = await import('../config.js');
    const deleted = deleteByMeta({ configKey: 'github' });
    expect(deleted).toEqual(['github']);
  });

  it('returns empty array when nothing to delete', async () => {
    fs.readFileSync = vi.fn(() => JSON.stringify({ mcpServers: {} }));
    fs.existsSync = vi.fn(() => true);
    fs.copyFileSync = vi.fn();
    fs.mkdirSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    const { deleteByMeta } = await import('../config.js');
    const deleted = deleteByMeta({ configKey: 'github' });
    expect(deleted).toEqual([]);
  });
});
