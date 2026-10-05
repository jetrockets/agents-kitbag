import { describe, it, expect, vi, beforeEach } from 'vitest';

beforeEach(() => {
  vi.resetModules();
  vi.restoreAllMocks();
});

describe('parseArgv', () => {
  it('splits secret bindings from the command after --', async () => {
    const { parseArgv } = await import('../secret-runner.js');
    const parsed = parseArgv([
      '--secret', 'ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana',
      '--', 'npx', '-y', 'pkg',
    ]);
    expect(parsed.secrets).toEqual([{ variable: 'ASANA_ACCESS_TOKEN', ref: 'keychain:claude-mcp-asana' }]);
    expect(parsed.command).toBe('npx');
    expect(parsed.args).toEqual(['-y', 'pkg']);
  });

  it('accepts several secrets', async () => {
    const { parseArgv } = await import('../secret-runner.js');
    const parsed = parseArgv([
      '--secret', 'A=keychain:one',
      '--secret', 'B=gh:',
      '--', 'server',
    ]);
    expect(parsed.secrets.map(s => s.variable)).toEqual(['A', 'B']);
  });

  // A server's own flags must not be mistaken for the runner's.
  it('leaves the server\'s own --secret flag alone', async () => {
    const { parseArgv } = await import('../secret-runner.js');
    const parsed = parseArgv(['--secret', 'A=gh:', '--', 'server', '--secret', 'not-ours']);
    expect(parsed.secrets).toHaveLength(1);
    expect(parsed.args).toEqual(['--secret', 'not-ours']);
  });

  it('rejects a binding without an equals sign', async () => {
    const { parseArgv } = await import('../secret-runner.js');
    expect(() => parseArgv(['--secret', 'nonsense', '--', 'server'])).toThrow(/VAR=/);
  });

  it('rejects a missing command', async () => {
    const { parseArgv } = await import('../secret-runner.js');
    expect(() => parseArgv(['--secret', 'A=gh:', '--'])).toThrow(/command/i);
  });

  it('rejects a missing -- separator', async () => {
    const { parseArgv } = await import('../secret-runner.js');
    expect(() => parseArgv(['--secret', 'A=gh:', 'server'])).toThrow(/--/);
  });
});

describe('resolveEnv', () => {
  it('puts each resolved secret under its variable name', async () => {
    vi.doMock('../secret-store.js', () => ({
      readSecret: async ref => (ref === 'keychain:one' ? 'first' : 'second'),
    }));
    const { resolveEnv } = await import('../secret-runner.js');

    const env = await resolveEnv([
      { variable: 'A', ref: 'keychain:one' },
      { variable: 'B', ref: 'gh:' },
    ]);

    expect(env).toEqual({ A: 'first', B: 'second' });
  });

  // Starting the server without its credential produces a confusing 401 later;
  // failing here says what actually went wrong.
  it('throws naming the variable when a secret cannot be read', async () => {
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async () => null,
      isBackendUsable: async () => true,
    }));
    const { resolveEnv } = await import('../secret-runner.js');

    await expect(resolveEnv([{ variable: 'ASANA_ACCESS_TOKEN', ref: 'keychain:missing' }]))
      .rejects.toThrow(/ASANA_ACCESS_TOKEN/);
  });

  it('never puts the secret itself in the error message', async () => {
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async () => null,
      isBackendUsable: async () => true,
    }));
    const { resolveEnv } = await import('../secret-runner.js');

    const err = await resolveEnv([{ variable: 'T', ref: 'keychain:x' }]).catch(e => e);
    expect(err.message).toContain('keychain:x');
  });
});

// A bare system PATH hides `gh` entirely; blaming a missing keychain entry
// sends the user looking in the wrong place.
describe('resolveEnv error wording', () => {
  it('says the backend is unavailable when its command is missing', async () => {
    vi.resetModules();
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async () => null,
      isBackendUsable: async () => false,
    }));
    const { resolveEnv } = await import('../secret-runner.js');

    const err = await resolveEnv([{ variable: 'T', ref: 'gh:' }]).catch(e => e);
    expect(err.message).toMatch(/GitHub CLI.*not available/i);
  });

  it('still blames the entry when the backend works', async () => {
    vi.resetModules();
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async () => null,
      isBackendUsable: async () => true,
    }));
    const { resolveEnv } = await import('../secret-runner.js');

    const err = await resolveEnv([{ variable: 'T', ref: 'keychain:x' }]).catch(e => e);
    expect(err.message).toMatch(/entry is missing/i);
  });
});
