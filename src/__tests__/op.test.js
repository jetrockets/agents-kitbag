import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'node:fs';

vi.mock('node:fs');

beforeEach(() => {
  vi.resetModules();
  vi.restoreAllMocks();
});

describe('isSecretRef', () => {
  it('recognises an op:// reference', async () => {
    const { isSecretRef } = await import('../op.js');
    expect(isSecretRef('op://Private/Claude MCP - Asana/credential')).toBe(true);
  });

  it('rejects a plain token', async () => {
    const { isSecretRef } = await import('../op.js');
    expect(isSecretRef('2/1199657140247831/abc')).toBe(false);
  });

  it('rejects undefined', async () => {
    const { isSecretRef } = await import('../op.js');
    expect(isSecretRef(undefined)).toBe(false);
  });
});

describe('secretRef', () => {
  it('builds a reference from vault and title', async () => {
    const { secretRef } = await import('../op.js');
    expect(secretRef('Private', 'Claude MCP - Asana')).toBe('op://Private/Claude MCP - Asana/credential');
  });

  it('accepts a custom field', async () => {
    const { secretRef } = await import('../op.js');
    expect(secretRef('Shared', 'Item', 'password')).toBe('op://Shared/Item/password');
  });
});

describe('itemTitle', () => {
  it('namespaces the config key so items are findable in 1Password', async () => {
    const { itemTitle } = await import('../op.js');
    expect(itemTitle('asana')).toBe('Claude MCP - asana');
  });

  it('keeps multi-instance keys distinct', async () => {
    const { itemTitle } = await import('../op.js');
    expect(itemTitle('jira-acme')).not.toBe(itemTitle('jira-corp'));
  });
});

describe('wrapWithOpRun', () => {
  it('puts op in front of the original command', async () => {
    const { wrapWithOpRun } = await import('../op.js');
    const wrapped = wrapWithOpRun(
      { command: 'npx', args: ['-y', 'pkg'], env: { TOKEN: 'op://v/i/credential' } },
      '/opt/homebrew/bin/op',
    );
    expect(wrapped.command).toBe('/opt/homebrew/bin/op');
    expect(wrapped.args).toEqual(['run', '--no-masking', '--', 'npx', '-y', 'pkg']);
  });

  // Without --no-masking, op rewrites any secret appearing on stdout to
  // "<concealed by 1Password>" — and stdout is the MCP server's JSON-RPC
  // transport, so masking corrupts the protocol.
  it('always disables masking', async () => {
    const { wrapWithOpRun } = await import('../op.js');
    const wrapped = wrapWithOpRun({ command: 'node', args: ['server.js'] }, '/usr/local/bin/op');
    expect(wrapped.args).toContain('--no-masking');
  });

  it('preserves env so op can resolve the references in it', async () => {
    const { wrapWithOpRun } = await import('../op.js');
    const wrapped = wrapWithOpRun(
      { command: 'npx', args: [], env: { FIGMA_API_KEY: 'op://Private/x/credential' } },
      '/opt/homebrew/bin/op',
    );
    expect(wrapped.env).toEqual({ FIGMA_API_KEY: 'op://Private/x/credential' });
  });

  it('handles a server config with no args', async () => {
    const { wrapWithOpRun } = await import('../op.js');
    const wrapped = wrapWithOpRun({ command: 'server-bin' }, '/opt/homebrew/bin/op');
    expect(wrapped.args).toEqual(['run', '--no-masking', '--', 'server-bin']);
  });

  it('does not double-wrap an already wrapped config', async () => {
    const { wrapWithOpRun } = await import('../op.js');
    const once = wrapWithOpRun({ command: 'npx', args: ['-y', 'pkg'] }, '/opt/homebrew/bin/op');
    const twice = wrapWithOpRun(once, '/opt/homebrew/bin/op');
    expect(twice).toEqual(once);
  });
});

describe('isOpWrapped', () => {
  it('detects a wrapped config', async () => {
    const { isOpWrapped } = await import('../op.js');
    expect(isOpWrapped({ command: '/opt/homebrew/bin/op', args: ['run', '--no-masking', '--', 'npx'] })).toBe(true);
  });

  it('detects op.exe on Windows', async () => {
    const { isOpWrapped } = await import('../op.js');
    expect(isOpWrapped({ command: 'C:\\Program Files\\1Password CLI\\op.exe', args: ['run', '--'] })).toBe(true);
  });

  it('rejects a plain config', async () => {
    const { isOpWrapped } = await import('../op.js');
    expect(isOpWrapped({ command: 'npx', args: ['-y', 'pkg'] })).toBe(false);
  });

  it('rejects a command merely containing "op"', async () => {
    const { isOpWrapped } = await import('../op.js');
    expect(isOpWrapped({ command: '/usr/bin/openssl', args: ['run'] })).toBe(false);
  });
});

describe('resolveOpPath on macOS and Linux', () => {
  // The candidate list is chosen from the platform at import time, so the
  // branch under test has to be pinned — CI runs this suite on Windows too.
  const withPosix = async existing => {
    vi.resetModules();
    vi.doMock('../platform.js', () => ({ isWindows: false, openBrowser: () => {} }));
    fs.existsSync = vi.fn(p => existing.includes(p));
    return (await import('../op.js')).resolveOpPath();
  };

  it('returns the first candidate that exists on disk', async () => {
    expect(await withPosix(['/opt/homebrew/bin/op'])).toBe('/opt/homebrew/bin/op');
  });

  it('prefers Homebrew over /usr/local when both are present', async () => {
    expect(await withPosix(['/opt/homebrew/bin/op', '/usr/local/bin/op'])).toBe('/opt/homebrew/bin/op');
  });

  it('returns null when op is nowhere to be found', async () => {
    expect(await withPosix([])).toBe(null);
  });

  // Claude Desktop launched from the Dock gets a bare system PATH, so a bare
  // "op" in the config would not resolve. Always write an absolute path.
  it('never returns a bare command name', async () => {
    expect(await withPosix(['/usr/local/bin/op'])).not.toBe('op');
  });
});

describe('itemTemplate', () => {
  it('puts the token in the credential field', async () => {
    const { itemTemplate } = await import('../op.js');
    const tpl = JSON.parse(itemTemplate('Claude MCP - asana', 'secret-token'));
    expect(tpl.category).toBe('API_CREDENTIAL');
    expect(tpl.title).toBe('Claude MCP - asana');
    expect(tpl.fields.find(f => f.id === 'credential').value).toBe('secret-token');
  });

  it('marks the credential field concealed', async () => {
    const { itemTemplate } = await import('../op.js');
    const tpl = JSON.parse(itemTemplate('t', 'v'));
    expect(tpl.fields.find(f => f.id === 'credential').type).toBe('CONCEALED');
  });
});

// resolveOpPath builds its candidate list from process.platform at import time,
// so the Windows branch needs the platform module stubbed.
describe('resolveOpPath on Windows', () => {
  const withWindows = async (env, existing) => {
    vi.resetModules();
    Object.assign(process.env, env);
    vi.doMock('../platform.js', () => ({ isWindows: true, openBrowser: () => {} }));
    fs.existsSync = vi.fn(p => existing.includes(p));
    return (await import('../op.js')).resolveOpPath();
  };

  it('finds the winget shim', async () => {
    const found = await withWindows(
      { LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local' },
      ['C:\\Users\\u\\AppData\\Local\\Microsoft\\WinGet\\Links\\op.exe'],
    );
    expect(found).toBe('C:\\Users\\u\\AppData\\Local\\Microsoft\\WinGet\\Links\\op.exe');
  });

  it('finds an MSI install under Program Files', async () => {
    const found = await withWindows(
      { LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local', ProgramFiles: 'C:\\Program Files' },
      ['C:\\Program Files\\1Password CLI\\op.exe'],
    );
    expect(found).toBe('C:\\Program Files\\1Password CLI\\op.exe');
  });

  it('finds a scoop shim', async () => {
    const found = await withWindows(
      { USERPROFILE: 'C:\\Users\\u' },
      ['C:\\Users\\u\\scoop\\shims\\op.exe'],
    );
    expect(found).toBe('C:\\Users\\u\\scoop\\shims\\op.exe');
  });

  it('finds a chocolatey shim', async () => {
    const found = await withWindows(
      { ProgramData: 'C:\\ProgramData' },
      ['C:\\ProgramData\\chocolatey\\bin\\op.exe'],
    );
    expect(found).toBe('C:\\ProgramData\\chocolatey\\bin\\op.exe');
  });

  // Claude Desktop spawns the command directly, and Node refuses to spawn a
  // .cmd/.bat without a shell (CVE-2024-27980), so a shim path must be the .exe.
  it('never points at a .cmd or .bat shim', async () => {
    const found = await withWindows(
      { USERPROFILE: 'C:\\Users\\u', ProgramData: 'C:\\ProgramData' },
      ['C:\\Users\\u\\scoop\\shims\\op.cmd', 'C:\\Users\\u\\scoop\\shims\\op.exe'],
    );
    expect(found.endsWith('.exe')).toBe(true);
  });

  it('returns null when 1Password CLI is not installed', async () => {
    const found = await withWindows({ LOCALAPPDATA: 'C:\\Users\\u\\AppData\\Local' }, []);
    expect(found).toBe(null);
  });
});

// With the 1Password desktop app integration there is no CLI session, so
// `op whoami` fails while reads succeed via biometric unlock. Detection has to
// probe an operation that actually touches the user's data.
describe('isUsable', () => {
  const withOpResult = async results => {
    vi.resetModules();
    vi.doMock('node:child_process', () => ({
      spawn: (bin, args) => {
        const listeners = {};
        const stream = { on: () => {} };
        setImmediate(() => listeners.close?.(results[args[0]] ?? 1));
        return {
          stdout: stream,
          stderr: stream,
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    fs.existsSync = vi.fn(() => true);
    return await import('../op.js');
  };

  it('reports usable when vaults can be listed even though whoami fails', async () => {
    const { isUsable } = await withOpResult({ vault: 0, whoami: 1 });
    expect(await isUsable('/opt/homebrew/bin/op')).toBe(true);
  });

  it('reports unusable when vaults cannot be listed', async () => {
    const { isUsable } = await withOpResult({ vault: 1 });
    expect(await isUsable('/opt/homebrew/bin/op')).toBe(false);
  });

  it('reports unusable when op is not installed', async () => {
    const { isUsable } = await withOpResult({});
    expect(await isUsable(null)).toBe(false);
  });
});

describe('createItem', () => {
  // The suite mocks node:fs wholesale; createItem needs the real one to write
  // its template, and so does this spy to read it back.
  const spyOnOp = async () => {
    const realFs = (await vi.importActual('node:fs')).default;
    const seen = { args: null, template: null };
    vi.doMock('node:child_process', () => ({
      spawn: (bin, args) => {
        seen.args = args;
        const file = args.find(a => a.startsWith('--template='))?.slice('--template='.length);
        seen.template = file ? realFs.readFileSync(file, 'utf8') : null;
        const listeners = {};
        setImmediate(() => listeners.close?.(0));
        return {
          stdout: { on: (e, cb) => { if (e === 'data') cb('{"title":"Claude MCP - asana"}'); } },
          stderr: { on: () => {} },
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    return seen;
  };

  beforeEach(() => {
    vi.resetModules();
    vi.doUnmock('node:fs');
  });

  // `op item create -` only honours a template arriving on a real pipe; Node's
  // stdio pipes are socketpairs, which op ignores while still creating an
  // "Untitled" item. Verified against op 2.39.0.
  it('passes the template as a file, not on stdin', async () => {
    const seen = await spyOnOp();
    const { createItem } = await import('../op.js');

    const ref = await createItem({ title: 'Claude MCP - asana', vault: 'Employee', token: 't', opBinary: '/bin/op' });

    expect(seen.args.some(a => a.startsWith('--template='))).toBe(true);
    expect(seen.args).not.toContain('-');
    expect(ref).toBe('op://Employee/Claude MCP - asana/credential');
  });

  it('writes the token into the template file, never into argv', async () => {
    const seen = await spyOnOp();
    const { createItem } = await import('../op.js');

    await createItem({ title: 't', vault: 'Employee', token: 'super-secret', opBinary: '/bin/op' });

    expect(seen.args.join(' ')).not.toContain('super-secret');
    expect(JSON.parse(seen.template).fields.find(f => f.id === 'credential').value).toBe('super-secret');
  });

  it('removes the template file even when op fails', async () => {
    let templatePath = null;
    vi.doMock('node:child_process', () => ({
      spawn: (bin, args) => {
        templatePath = args.find(a => a.startsWith('--template='))?.slice('--template='.length);
        const listeners = {};
        setImmediate(() => {
          listeners.close?.(1);
        });
        return {
          stdout: { on: () => {} },
          stderr: { on: (e, cb) => { if (e === 'data') cb('vault is read-only'); } },
          stdin: { end: () => {} },
          on: (evt, cb) => { listeners[evt] = cb; },
        };
      },
    }));
    const realFs = (await vi.importActual('node:fs')).default;
    const { createItem } = await import('../op.js');

    await expect(createItem({ title: 't', vault: 'v', token: 'x', opBinary: '/bin/op' }))
      .rejects.toThrow(/read-only/);
    expect(realFs.existsSync(templatePath)).toBe(false);
  });
});
