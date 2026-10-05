import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

vi.mock('node:fs');

const { backupConfig, purgeBackups, RETENTION } = await import('../backup.js');

beforeEach(() => {
  vi.restoreAllMocks();
});

describe('backupConfig', () => {

  it('does nothing when config file does not exist', () => {
    fs.existsSync = vi.fn(() => false);
    backupConfig('/fake/path/config.json');
    expect(fs.copyFileSync).not.toHaveBeenCalled();
  });

  it('creates backup directory and copies file', () => {
    fs.existsSync = vi.fn(() => true);
    fs.mkdirSync = vi.fn();
    fs.copyFileSync = vi.fn();
    fs.readdirSync = vi.fn(() => []);

    backupConfig('/fake/path/config.json');

    expect(fs.mkdirSync).toHaveBeenCalledWith(
      path.join('/fake/path', 'backups'),
      { recursive: true },
    );
    expect(fs.copyFileSync).toHaveBeenCalled();
  });

  it('prunes backups down to the retention limit', () => {
    fs.existsSync = vi.fn(() => true);
    fs.mkdirSync = vi.fn();
    fs.copyFileSync = vi.fn();
    fs.unlinkSync = vi.fn();

    const backups = Array.from({ length: 15 }, (_, i) =>
      `claude_desktop_config_${String(i).padStart(15, '0')}.json`,
    );
    fs.readdirSync = vi.fn(() => backups);

    backupConfig('/fake/path/config.json');

    expect(fs.unlinkSync).toHaveBeenCalledTimes(15 - RETENTION);
  });

  // Backups hold the same plaintext tokens as the config, so keeping ten
  // generations means a revoked token survives on disk long after rotation.
  it('keeps few enough generations to limit token exposure', () => {
    expect(RETENTION).toBeLessThanOrEqual(3);
  });

  it('deletes the newest backups last', () => {
    fs.existsSync = vi.fn(() => true);
    fs.mkdirSync = vi.fn();
    fs.copyFileSync = vi.fn();
    fs.unlinkSync = vi.fn();
    fs.readdirSync = vi.fn(() => [
      'claude_desktop_config_20260101000000.json',
      'claude_desktop_config_20260601000000.json',
      'claude_desktop_config_20260901000000.json',
      'claude_desktop_config_20260902000000.json',
    ]);

    backupConfig('/fake/path/config.json');

    const deleted = fs.unlinkSync.mock.calls.map(c => c[0]);
    expect(deleted).toContain(path.join('/fake/path', 'backups', 'claude_desktop_config_20260101000000.json'));
    expect(deleted).not.toContain(path.join('/fake/path', 'backups', 'claude_desktop_config_20260902000000.json'));
  });
});

describe('purgeBackups on Windows', () => {
  it('clears the read-only flag a Windows backup can inherit from the config', async () => {
    vi.resetModules();
    vi.doMock('../platform.js', () => ({ isWindows: true, openBrowser: () => {} }));
    const { purgeBackups: purgeOnWindows } = await import('../backup.js');

    fs.existsSync = vi.fn(() => true);
    fs.chmodSync = vi.fn();
    let firstAttempt = true;
    fs.unlinkSync = vi.fn(() => {
      if (firstAttempt) {
        firstAttempt = false;
        const err = new Error('EPERM: operation not permitted');
        err.code = 'EPERM';
        throw err;
      }
    });
    fs.readdirSync = vi.fn(() => ['claude_desktop_config_20260101000000.json']);

    expect(purgeOnWindows('/fake/path/config.json')).toBe(1);
    expect(fs.chmodSync).toHaveBeenCalledWith(
      path.join('/fake/path', 'backups', 'claude_desktop_config_20260101000000.json'),
      0o666,
    );
  });

  it('still surfaces a non-EPERM failure', async () => {
    vi.resetModules();
    vi.doMock('../platform.js', () => ({ isWindows: true, openBrowser: () => {} }));
    const { purgeBackups: purgeOnWindows } = await import('../backup.js');

    fs.existsSync = vi.fn(() => true);
    fs.readdirSync = vi.fn(() => ['claude_desktop_config_20260101000000.json']);
    fs.unlinkSync = vi.fn(() => {
      const err = new Error('EBUSY: resource busy');
      err.code = 'EBUSY';
      throw err;
    });

    expect(() => purgeOnWindows('/fake/path/config.json')).toThrow(/EBUSY/);
  });
});

describe('purgeBackups', () => {
  it('deletes every backup and reports the count', () => {
    fs.existsSync = vi.fn(() => true);
    fs.unlinkSync = vi.fn();
    fs.readdirSync = vi.fn(() => [
      'claude_desktop_config_20260101000000.json',
      'claude_desktop_config_20260601000000.json',
      'unrelated.txt',
    ]);

    expect(purgeBackups('/fake/path/config.json')).toBe(2);
    expect(fs.unlinkSync).toHaveBeenCalledTimes(2);
  });

  it('leaves unrelated files in the backups directory alone', () => {
    fs.existsSync = vi.fn(() => true);
    fs.unlinkSync = vi.fn();
    fs.readdirSync = vi.fn(() => ['notes.md']);

    expect(purgeBackups('/fake/path/config.json')).toBe(0);
    expect(fs.unlinkSync).not.toHaveBeenCalled();
  });

  it('returns 0 when there is no backups directory', () => {
    fs.existsSync = vi.fn(() => false);
    expect(purgeBackups('/fake/path/config.json')).toBe(0);
  });
});
