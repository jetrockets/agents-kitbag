import { describe, it, expect, vi, beforeEach } from 'vitest';

beforeEach(() => {
  vi.resetModules();
  vi.restoreAllMocks();
});

const onPlatform = async platform => {
  vi.resetModules();
  vi.doMock('../platform.js', () => ({ isWindows: platform === 'win32', openBrowser: () => {} }));
  vi.stubGlobal('process', { ...process, platform });
  return await import('../secret-store.js');
};

describe('parseRef', () => {
  it('splits a backend prefix from its name', async () => {
    const { parseRef } = await import('../secret-store.js');
    expect(parseRef('keychain:claude-mcp-asana')).toEqual({ backend: 'keychain', name: 'claude-mcp-asana' });
  });

  it('accepts a backend with an empty name', async () => {
    const { parseRef } = await import('../secret-store.js');
    expect(parseRef('gh:')).toEqual({ backend: 'gh', name: '' });
  });

  it('keeps colons that belong to the name', async () => {
    const { parseRef } = await import('../secret-store.js');
    expect(parseRef('keychain:a:b')).toEqual({ backend: 'keychain', name: 'a:b' });
  });

  it('rejects a string with no backend prefix', async () => {
    const { parseRef } = await import('../secret-store.js');
    expect(() => parseRef('just-a-value')).toThrow(/backend/i);
  });

  it('rejects an unknown backend', async () => {
    const { parseRef } = await import('../secret-store.js');
    expect(() => parseRef('carrier-pigeon:x')).toThrow(/unknown backend/i);
  });
});

describe('secretRefFor', () => {
  it('names a keychain entry after the config key', async () => {
    const { secretRefFor } = await import('../secret-store.js');
    expect(secretRefFor('keychain', 'asana')).toBe('keychain:claude-mcp-asana');
  });

  it('keeps multi-instance keys distinct', async () => {
    const { secretRefFor } = await import('../secret-store.js');
    expect(secretRefFor('keychain', 'jira-acme')).not.toBe(secretRefFor('keychain', 'jira-corp'));
  });

  // The GitHub CLI already holds the token; there is nothing to store.
  it('needs no name for the gh backend', async () => {
    const { secretRefFor } = await import('../secret-store.js');
    expect(secretRefFor('gh', 'github')).toBe('gh:');
  });
});

describe('defaultBackend', () => {
  it('is the Keychain on macOS', async () => {
    const { defaultBackend } = await onPlatform('darwin');
    expect(defaultBackend()).toBe('keychain');
  });

  it('is DPAPI on Windows', async () => {
    const { defaultBackend } = await onPlatform('win32');
    expect(defaultBackend()).toBe('dpapi');
  });

  it('is libsecret on Linux', async () => {
    const { defaultBackend } = await onPlatform('linux');
    expect(defaultBackend()).toBe('libsecret');
  });
});

describe('backendLabel', () => {
  it('names each backend in words a user recognises', async () => {
    const { backendLabel } = await import('../secret-store.js');
    expect(backendLabel('keychain')).toMatch(/keychain/i);
    expect(backendLabel('dpapi')).toMatch(/windows/i);
    expect(backendLabel('libsecret')).toMatch(/secret service|libsecret/i);
    expect(backendLabel('gh')).toMatch(/github cli/i);
  });
});

// Reading a secret must never put it in argv, where any process can see it.
describe('readSecret argument safety', () => {
  const spawnSpy = () => {
    const seen = [];
    vi.doMock('node:child_process', () => ({
      spawn: (bin, args) => {
        seen.push({ bin, args });
        const listeners = {};
        setImmediate(() => listeners.close?.(0));
        return {
          stdout: { on: (e, cb) => { if (e === 'data') cb('the-secret\n'); } },
          stderr: { on: () => {} },
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    return seen;
  };

  it('asks the Keychain for the entry by service name', async () => {
    const seen = spawnSpy();
    const { readSecret } = await import('../secret-store.js');

    expect(await readSecret('keychain:claude-mcp-asana')).toBe('the-secret');
    expect(seen[0].args).toContain('claude-mcp-asana');
    expect(seen[0].args).toContain('-w');
  });

  it('shells out to the GitHub CLI for the gh backend', async () => {
    const seen = spawnSpy();
    const { readSecret } = await import('../secret-store.js');

    expect(await readSecret('gh:')).toBe('the-secret');
    expect(seen[0].args).toEqual(['auth', 'token']);
  });

  it('returns null when the backend command fails', async () => {
    vi.doMock('node:child_process', () => ({
      spawn: () => {
        const listeners = {};
        setImmediate(() => listeners.close?.(1));
        return {
          stdout: { on: () => {} },
          stderr: { on: (e, cb) => { if (e === 'data') cb('not found'); } },
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    const { readSecret } = await import('../secret-store.js');

    expect(await readSecret('keychain:missing')).toBe(null);
  });
});

describe('storeSecret argument safety', () => {
  it('never passes the secret as a command argument', async () => {
    const seen = [];
    vi.doMock('node:child_process', () => ({
      spawn: (bin, args) => {
        seen.push({ bin, args });
        const listeners = {};
        setImmediate(() => listeners.close?.(0));
        return {
          stdout: { on: () => {} },
          stderr: { on: () => {} },
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    const { storeSecret } = await import('../secret-store.js');

    await storeSecret('keychain:claude-mcp-asana', 'super-secret');

    expect(seen.every(c => !c.args.join(' ').includes('super-secret'))).toBe(true);
  });
});

// Claude Desktop launched from the Dock inherits a bare system PATH, so a bare
// `gh` or `secret-tool` does not resolve and the server never starts.
describe('backend executables', () => {
  const spawnSpy = existing => {
    const seen = [];
    vi.doMock('node:fs', async () => {
      const actual = await vi.importActual('node:fs');
      return { ...actual, default: { ...actual.default, existsSync: p => existing.includes(p) } };
    });
    vi.doMock('node:child_process', () => ({
      spawn: (bin, args) => {
        seen.push(bin);
        const listeners = {};
        setImmediate(() => listeners.close?.(0));
        return {
          stdout: { on: (e, cb) => { if (e === 'data') cb('secret\n'); } },
          stderr: { on: () => {} },
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    return seen;
  };

  it('calls gh by absolute path when Homebrew has it', async () => {
    vi.resetModules();
    const seen = spawnSpy(['/opt/homebrew/bin/gh']);
    const { readSecret } = await import('../secret-store.js');

    await readSecret('gh:');

    expect(seen[0]).toBe('/opt/homebrew/bin/gh');
  });

  it('falls back to /usr/local/bin for an Intel install', async () => {
    vi.resetModules();
    const seen = spawnSpy(['/usr/local/bin/gh']);
    const { readSecret } = await import('../secret-store.js');

    await readSecret('gh:');

    expect(seen[0]).toBe('/usr/local/bin/gh');
  });

  it('calls security by absolute path', async () => {
    vi.resetModules();
    const seen = spawnSpy(['/usr/bin/security']);
    const { readSecret } = await import('../secret-store.js');

    await readSecret('keychain:whatever');

    expect(seen[0]).toBe('/usr/bin/security');
  });

  it('calls secret-tool by absolute path', async () => {
    vi.resetModules();
    const seen = spawnSpy(['/usr/bin/secret-tool']);
    const { readSecret } = await import('../secret-store.js');

    await readSecret('libsecret:whatever');

    expect(seen[0]).toBe('/usr/bin/secret-tool');
  });

  // Better to try the bare name than to give up: a user may have it elsewhere.
  it('falls back to the bare name when no candidate exists', async () => {
    vi.resetModules();
    const seen = spawnSpy([]);
    const { readSecret } = await import('../secret-store.js');

    await readSecret('gh:');

    expect(seen[0]).toBe('gh');
  });
});
