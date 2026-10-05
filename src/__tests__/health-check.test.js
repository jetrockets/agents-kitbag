import { describe, it, expect, vi } from 'vitest';
import { extractBearerToken, classifyResponse } from '../health-check.js';

describe('extractBearerToken', () => {
  it('extracts token from --header arg', () => {
    const config = {
      args: ['-y', 'mcp-remote', 'https://mcp.linear.app/mcp', '--header', 'Authorization: Bearer lin_api_abc123'],
    };
    expect(extractBearerToken(config)).toBe('lin_api_abc123');
  });

  it('extracts a GitHub remote token from --header arg', () => {
    const config = {
      args: ['-y', 'mcp-remote', 'https://api.githubcopilot.com/mcp/', '--header', 'Authorization: Bearer gho_xyz789'],
    };
    expect(extractBearerToken(config)).toBe('gho_xyz789');
  });

  it('returns null when no --header flag', () => {
    const config = { args: ['-y', 'mcp-remote', 'https://mcp.linear.app/mcp'] };
    expect(extractBearerToken(config)).toBeNull();
  });

  it('returns null when --header is last arg (no value)', () => {
    const config = { args: ['--header'] };
    expect(extractBearerToken(config)).toBeNull();
  });

  it('returns null when header value is not Authorization', () => {
    const config = { args: ['--header', 'X-Custom: value'] };
    expect(extractBearerToken(config)).toBeNull();
  });

  it('returns null when args is undefined', () => {
    expect(extractBearerToken({})).toBeNull();
  });

  it('returns null when args is empty', () => {
    expect(extractBearerToken({ args: [] })).toBeNull();
  });

  it('handles case-insensitive Authorization', () => {
    const config = { args: ['--header', 'authorization: bearer tok123'] };
    expect(extractBearerToken(config)).toBe('tok123');
  });
});

describe('classifyResponse', () => {
  it('returns ok for successful response', () => {
    expect(classifyResponse({ ok: true, status: 200 })).toEqual({ status: 'ok', detail: null });
  });

  it('returns expired for 401', () => {
    expect(classifyResponse({ ok: false, status: 401 })).toEqual({ status: 'expired', detail: 'HTTP 401' });
  });

  it('returns expired for 403', () => {
    expect(classifyResponse({ ok: false, status: 403 })).toEqual({ status: 'expired', detail: 'HTTP 403' });
  });

  it('returns error for 500', () => {
    expect(classifyResponse({ ok: false, status: 500 })).toEqual({ status: 'error', detail: 'HTTP 500' });
  });

  it('returns error for 404', () => {
    expect(classifyResponse({ ok: false, status: 404 })).toEqual({ status: 'error', detail: 'HTTP 404' });
  });

  it('returns error for null (network failure)', () => {
    expect(classifyResponse(null)).toEqual({ status: 'error', detail: 'network error' });
  });
});

describe('resolveSecret', () => {
  it('passes a plain token through untouched', async () => {
    const { resolveSecret } = await import('../health-check.js');
    expect(await resolveSecret({}, 'plain-token')).toBe('plain-token');
  });

  it('returns null for a missing value', async () => {
    const { resolveSecret } = await import('../health-check.js');
    expect(await resolveSecret({}, undefined)).toBeNull();
  });

  // GitHub and Linear put ${VAR} in the Authorization header and the op://
  // reference in env, so a health check has to follow the indirection.
  it('follows a ${VAR} placeholder into env', async () => {
    vi.resetModules();
    vi.doMock('../op.js', async () => ({
      ...(await vi.importActual('../op.js')),
      readSecret: async ref => (ref === 'op://Private/i/credential' ? 'real-token' : null),
    }));
    const { resolveSecret } = await import('../health-check.js');

    const config = { env: { GITHUB_MCP_TOKEN: 'op://Private/i/credential' } };
    expect(await resolveSecret(config, '${GITHUB_MCP_TOKEN}')).toBe('real-token');
  });

  it('reads an op:// reference through the 1Password CLI', async () => {
    vi.resetModules();
    vi.doMock('../op.js', async () => ({
      ...(await vi.importActual('../op.js')),
      readSecret: async () => 'real-token',
    }));
    const { resolveSecret } = await import('../health-check.js');

    expect(await resolveSecret({}, 'op://Private/i/credential')).toBe('real-token');
  });

  it('returns null when 1Password is locked or the item is gone', async () => {
    vi.resetModules();
    vi.doMock('../op.js', async () => ({
      ...(await vi.importActual('../op.js')),
      readSecret: async () => null,
    }));
    const { resolveSecret } = await import('../health-check.js');

    expect(await resolveSecret({}, 'op://Private/i/credential')).toBeNull();
  });
});

describe('checkAllTokens with 1Password-backed servers', () => {
  it('reports a locked 1Password instead of a misleading "token missing"', async () => {
    vi.resetModules();
    vi.doMock('../config.js', () => ({
      readConfig: () => ({
        mcpServers: {
          asana: {
            command: '/opt/homebrew/bin/op',
            args: ['run', '--no-masking', '--', 'npx', '-y', 'pkg'],
            env: { ASANA_ACCESS_TOKEN: 'op://Private/i/credential' },
          },
        },
      }),
    }));
    vi.doMock('../op.js', async () => ({
      ...(await vi.importActual('../op.js')),
      isUsable: async () => false,
    }));
    const { checkAllTokens } = await import('../health-check.js');

    const results = await checkAllTokens();
    expect(results).toEqual([{ key: 'asana', status: 'skip', detail: '1Password locked — unlock to check' }]);
  });

  it('checks the token normally when 1Password is unlocked', async () => {
    vi.resetModules();
    vi.doMock('../config.js', () => ({
      readConfig: () => ({
        mcpServers: {
          asana: {
            command: '/opt/homebrew/bin/op',
            args: ['run', '--no-masking', '--', 'npx', '-y', 'pkg'],
            env: { ASANA_ACCESS_TOKEN: 'op://Private/i/credential' },
          },
        },
      }),
    }));
    vi.doMock('../op.js', async () => ({
      ...(await vi.importActual('../op.js')),
      isUsable: async () => true,
      readSecret: async () => 'real-token',
    }));
    vi.doMock('../network.js', () => ({ safeFetch: async () => ({ ok: true, status: 200 }) }));
    const { checkAllTokens } = await import('../health-check.js');

    const results = await checkAllTokens();
    expect(results[0]).toEqual({ key: 'asana', status: 'ok', detail: null });
  });

  it('leaves plaintext servers alone when 1Password is absent', async () => {
    vi.resetModules();
    vi.doMock('../config.js', () => ({
      readConfig: () => ({
        mcpServers: { asana: { command: 'npx', args: [], env: { ASANA_ACCESS_TOKEN: 'plain' } } },
      }),
    }));
    vi.doMock('../op.js', async () => ({
      ...(await vi.importActual('../op.js')),
      isUsable: async () => false,
    }));
    vi.doMock('../network.js', () => ({ safeFetch: async () => ({ ok: true, status: 200 }) }));
    const { checkAllTokens } = await import('../health-check.js');

    const results = await checkAllTokens();
    expect(results[0].status).toBe('ok');
  });
});

describe('health checks for runner-backed servers', () => {
  const runnerConfig = {
    command: '/usr/local/bin/node',
    args: [
      '/opt/toolkit/src/secret-runner.js',
      '--secret', 'ASANA_ACCESS_TOKEN=keychain:claude-mcp-asana',
      '--', 'npx', '-y', 'pkg',
    ],
    env: {},
  };

  it('follows a --secret binding into the credential store', async () => {
    vi.resetModules();
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async ref => (ref === 'keychain:claude-mcp-asana' ? 'real-token' : null),
    }));
    const { resolveSecret } = await import('../health-check.js');

    expect(await resolveSecret(runnerConfig, undefined)).toBe('real-token');
  });

  it('checks the token normally once the store resolves it', async () => {
    vi.resetModules();
    vi.doMock('../config.js', () => ({ readConfig: () => ({ mcpServers: { asana: runnerConfig } }) }));
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async () => 'real-token',
    }));
    vi.doMock('../network.js', () => ({ safeFetch: async () => ({ ok: true, status: 200 }) }));
    const { checkAllTokens } = await import('../health-check.js');

    expect((await checkAllTokens())[0]).toEqual({ key: 'asana', status: 'ok', detail: null });
  });

  // A missing keychain entry means the server will not start at all; saying so
  // beats reporting an expired token.
  it('reports an unreadable store rather than a missing token', async () => {
    vi.resetModules();
    vi.doMock('../config.js', () => ({ readConfig: () => ({ mcpServers: { asana: runnerConfig } }) }));
    vi.doMock('../secret-store.js', async () => ({
      ...(await vi.importActual('../secret-store.js')),
      readSecret: async () => null,
    }));
    const { checkAllTokens } = await import('../health-check.js');

    const result = (await checkAllTokens())[0];
    expect(result.status).toBe('skip');
    expect(result.detail).toMatch(/credential store/i);
  });
});
