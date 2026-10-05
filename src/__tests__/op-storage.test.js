import { describe, it, expect, vi, beforeEach } from 'vitest';

const prompt = vi.fn();
vi.mock('inquirer', () => ({ default: { prompt: (...a) => prompt(...a) } }));

const opMock = {
  resolveOpPath: vi.fn(),
  isUsable: vi.fn(),
  listVaults: vi.fn(),
  createItem: vi.fn(),
};
vi.mock('../op.js', async () => {
  const actual = await vi.importActual('../op.js');
  return { ...actual, ...opMock };
});

const storeMock = {
  isBackendUsable: vi.fn(),
  storeSecret: vi.fn(),
  defaultBackend: vi.fn(),
};
vi.mock('../secret-store.js', async () => {
  const actual = await vi.importActual('../secret-store.js');
  return { ...actual, ...storeMock };
});

const answerStore = value => prompt.mockResolvedValueOnce({ store: value });

beforeEach(() => {
  vi.resetModules();
  prompt.mockReset();
  for (const fn of [...Object.values(opMock), ...Object.values(storeMock)]) fn.mockReset();
  opMock.resolveOpPath.mockReturnValue('/opt/homebrew/bin/op');
  opMock.isUsable.mockResolvedValue(true);
  opMock.listVaults.mockResolvedValue(['Private', 'Shared']);
  opMock.createItem.mockResolvedValue('op://Private/Claude MCP - asana/credential');
  storeMock.defaultBackend.mockReturnValue('keychain');
  storeMock.isBackendUsable.mockResolvedValue(true);
  storeMock.storeSecret.mockResolvedValue(undefined);
  vi.spyOn(console, 'log').mockImplementation(() => {});
});

describe('offerSecretStorage — what it offers', () => {
  it('offers 1Password, the system store and plain text when all are available', async () => {
    answerStore('plain');
    const { offerSecretStorage } = await import('../op-storage.js');

    await offerSecretStorage('asana', 'plain-token');

    const values = prompt.mock.calls[0][0][0].choices.map(c => c.value);
    expect(values).toEqual(['op', 'system', 'plain']);
  });

  it('drops 1Password from the list when the CLI cannot reach the vaults', async () => {
    opMock.isUsable.mockResolvedValue(false);
    answerStore('plain');
    const { offerSecretStorage } = await import('../op-storage.js');

    await offerSecretStorage('asana', 'plain-token');

    expect(prompt.mock.calls[0][0][0].choices.map(c => c.value)).toEqual(['system', 'plain']);
  });

  it('does not ask at all when no credential store is available', async () => {
    opMock.resolveOpPath.mockReturnValue(null);
    storeMock.isBackendUsable.mockResolvedValue(false);
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');

    expect(prompt).not.toHaveBeenCalled();
    expect(stored.value).toBe('plain-token');
  });

  it('keeps the token in the config when the user picks plain text', async () => {
    answerStore('plain');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');

    expect(stored.value).toBe('plain-token');
    expect(opMock.createItem).not.toHaveBeenCalled();
    expect(storeMock.storeSecret).not.toHaveBeenCalled();
  });
});

describe('offerSecretStorage — 1Password', () => {
  it('stores the token and returns a secret reference', async () => {
    answerStore('op');
    prompt.mockResolvedValueOnce({ vault: 'Private' });
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');

    expect(opMock.createItem).toHaveBeenCalledWith(expect.objectContaining({
      title: 'Claude MCP - asana', vault: 'Private', token: 'plain-token',
    }));
    expect(stored.value).toBe('op://Private/Claude MCP - asana/credential');
  });

  it('wraps the server config in op run', async () => {
    answerStore('op');
    prompt.mockResolvedValueOnce({ vault: 'Private' });
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');
    const wrapped = stored.apply({ command: 'npx', args: ['-y', 'pkg'], env: { ASANA_ACCESS_TOKEN: stored.value } });

    expect(wrapped.command).toBe('/opt/homebrew/bin/op');
    expect(wrapped.args).toEqual(['run', '--no-masking', '--', 'npx', '-y', 'pkg']);
  });

  it('falls back to plain text when 1Password refuses the item', async () => {
    answerStore('op');
    prompt.mockResolvedValueOnce({ vault: 'Private' });
    opMock.createItem.mockRejectedValue(new Error('vault is read-only'));
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');

    expect(stored.value).toBe('plain-token');
  });
});

describe('offerSecretStorage — system credential store', () => {
  it('stores the token under a name derived from the config key', async () => {
    answerStore('system');
    const { offerSecretStorage } = await import('../op-storage.js');

    await offerSecretStorage('asana', 'plain-token');

    expect(storeMock.storeSecret).toHaveBeenCalledWith('keychain:claude-mcp-asana', 'plain-token');
  });

  // The runner exists because Windows has no `sh -c`; it must be launched by
  // an absolute node path for the same reason op is.
  it('launches the server through the runner with an absolute node path', async () => {
    answerStore('system');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');
    const wrapped = stored.apply({ command: 'npx', args: ['-y', 'pkg'], env: { ASANA_ACCESS_TOKEN: stored.value } });

    expect(wrapped.command).toBe(process.execPath);
    expect(wrapped.args[0]).toMatch(/secret-runner\.js$/);
    expect(wrapped.args).toContain('--secret');
    expect(wrapped.args).toContain('ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana');
    expect(wrapped.args.slice(-3)).toEqual(['npx', '-y', 'pkg']);
  });

  it('removes the placeholder from env so no marker is left behind', async () => {
    answerStore('system');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');
    const wrapped = stored.apply({
      command: 'npx',
      args: [],
      env: { ASANA_ACCESS_TOKEN: stored.value, TOOLSETS: 'default' },
    });

    expect(wrapped.env).toEqual({ TOOLSETS: 'default' });
    expect(JSON.stringify(wrapped)).not.toContain('__CLAUDE_TOOLKIT_SECRET__');
  });

  it('never writes the token itself into the config', async () => {
    answerStore('system');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');
    const wrapped = stored.apply({ command: 'npx', args: [], env: { ASANA_ACCESS_TOKEN: stored.value } });

    expect(JSON.stringify(wrapped)).not.toContain('plain-token');
  });

  it('falls back to plain text when the store rejects the secret', async () => {
    answerStore('system');
    storeMock.storeSecret.mockRejectedValue(new Error('keychain is locked'));
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('asana', 'plain-token');

    expect(stored.value).toBe('plain-token');
    expect(stored.apply({ command: 'npx' })).toEqual({ command: 'npx' });
  });
});

// GitHub and Linear pass the token inside `args` as an Authorization header.
describe('offerSecretStorage — envVar mode', () => {
  it('returns a placeholder the wrapper will fill in', async () => {
    answerStore('system');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('github', 'gh-token', { envVar: 'GITHUB_MCP_TOKEN' });

    expect(stored.value).toBe('${GITHUB_MCP_TOKEN}');
  });

  it('binds that variable on the runner command line', async () => {
    answerStore('system');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('github', 'gh-token', { envVar: 'GITHUB_MCP_TOKEN' });
    const wrapped = stored.apply({
      command: 'npx',
      args: ['-y', 'mcp-remote', 'https://example.test/mcp', '--header', `Authorization: Bearer ${stored.value}`],
      env: {},
    });

    expect(wrapped.args).toContain('GITHUB_MCP_TOKEN=keychain:claude-mcp-github');
    expect(wrapped.args).toContain('Authorization: Bearer ${GITHUB_MCP_TOKEN}');
    expect(wrapped.env).toEqual({});
  });

  it('still puts an op:// reference in env when 1Password is chosen', async () => {
    answerStore('op');
    prompt.mockResolvedValueOnce({ vault: 'Private' });
    opMock.createItem.mockResolvedValue('op://Private/Claude MCP - github/credential');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('github', 'gh-token', { envVar: 'GITHUB_MCP_TOKEN' });
    const wrapped = stored.apply({ command: 'npx', args: [], env: {} });

    expect(wrapped.env.GITHUB_MCP_TOKEN).toBe('op://Private/Claude MCP - github/credential');
  });

  it('returns the real token when the user picks plain text', async () => {
    answerStore('plain');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('github', 'gh-token', { envVar: 'GITHUB_MCP_TOKEN' });

    expect(stored.value).toBe('gh-token');
  });
});

// For GitHub the CLI already holds the token; keeping a second copy is worse
// than asking gh for it at launch.
describe('offerSecretStorage — GitHub CLI as the source', () => {
  it('offers gh first when the integration allows it', async () => {
    storeMock.isBackendUsable.mockResolvedValue(true);
    answerStore('plain');
    const { offerSecretStorage } = await import('../op-storage.js');

    await offerSecretStorage('github', 'gh-token', { envVar: 'GITHUB_MCP_TOKEN', ghSource: true });

    expect(prompt.mock.calls[0][0][0].choices[0].value).toBe('gh');
  });

  it('is not offered to integrations that have no gh token', async () => {
    answerStore('plain');
    const { offerSecretStorage } = await import('../op-storage.js');

    await offerSecretStorage('asana', 'token');

    expect(prompt.mock.calls[0][0][0].choices.map(c => c.value)).not.toContain('gh');
  });

  it('stores nothing and binds the variable to the gh backend', async () => {
    answerStore('gh');
    const { offerSecretStorage } = await import('../op-storage.js');

    const stored = await offerSecretStorage('github', 'gh-token', { envVar: 'GITHUB_MCP_TOKEN', ghSource: true });
    const wrapped = stored.apply({ command: 'npx', args: ['-y', 'mcp-remote'], env: {} });

    expect(storeMock.storeSecret).not.toHaveBeenCalled();
    expect(opMock.createItem).not.toHaveBeenCalled();
    expect(wrapped.args).toContain('GITHUB_MCP_TOKEN=gh:');
    expect(JSON.stringify(wrapped)).not.toContain('gh-token');
  });
});
