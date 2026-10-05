import { describe, it, expect, vi } from 'vitest';
import { safeFetch } from '../network.js';

describe('safeFetch', () => {
  it('returns response on success', async () => {
    const mockResp = { ok: true, json: () => ({}) };
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(mockResp)));
    const result = await safeFetch('https://example.com');
    expect(result).toBe(mockResp);
    vi.unstubAllGlobals();
  });

  it('returns null on ENOTFOUND', async () => {
    const err = new Error('getaddrinfo ENOTFOUND');
    err.code = 'ENOTFOUND';
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(err)));
    const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const result = await safeFetch('https://bad.invalid');
    expect(result).toBeNull();
    expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('DNS lookup failed'));
    consoleSpy.mockRestore();
    vi.unstubAllGlobals();
  });

  it('returns null on ECONNREFUSED', async () => {
    const err = new Error('connect ECONNREFUSED');
    err.code = 'ECONNREFUSED';
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(err)));
    const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const result = await safeFetch('https://localhost:9999');
    expect(result).toBeNull();
    expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('Connection refused'));
    consoleSpy.mockRestore();
    vi.unstubAllGlobals();
  });

  it('returns null with generic message on other errors', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('timeout'))));
    const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const result = await safeFetch('https://example.com');
    expect(result).toBeNull();
    expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('Network error'));
    consoleSpy.mockRestore();
    vi.unstubAllGlobals();
  });

  it('aborts hung requests and returns null after the timeout', async () => {
    vi.stubGlobal('fetch', vi.fn((_url, opts) => new Promise((_resolve, reject) => {
      opts.signal.addEventListener('abort', () => {
        const err = new Error('aborted');
        err.name = 'AbortError';
        reject(err);
      });
    })));
    const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const result = await safeFetch('https://example.com', { timeout: 20 });
    expect(result).toBeNull();
    expect(consoleSpy).toHaveBeenCalledWith(expect.stringContaining('Network timeout'));
    consoleSpy.mockRestore();
    vi.unstubAllGlobals();
  });
});
