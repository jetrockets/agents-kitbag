import { describe, it, expect } from 'vitest';
import { parsePackageName } from '../npm.js';

describe('parsePackageName', () => {
  it('returns plain package name as-is', () => {
    expect(parsePackageName('mcp-server-github')).toBe('mcp-server-github');
  });

  it('strips version from unscoped package', () => {
    expect(parsePackageName('mcp-server-github@1.2.3')).toBe('mcp-server-github');
  });

  it('strips tag from unscoped package', () => {
    expect(parsePackageName('mcp-server-github@beta')).toBe('mcp-server-github');
  });

  it('handles scoped package without version', () => {
    expect(parsePackageName('@modelcontextprotocol/server-github')).toBe('@modelcontextprotocol/server-github');
  });

  it('strips version from scoped package', () => {
    expect(parsePackageName('@roychri/mcp-server-asana@beta')).toBe('@roychri/mcp-server-asana');
  });

  it('handles scope-only (edge case)', () => {
    expect(parsePackageName('@scope')).toBe('@scope');
  });
});
