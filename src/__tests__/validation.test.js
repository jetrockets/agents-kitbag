import { describe, it, expect } from 'vitest';
import { validateName, stripUrl } from '../validation.js';

describe('validateName', () => {
  it('accepts simple lowercase name', () => {
    expect(validateName('acme')).toBe(true);
  });

  it('accepts name with hyphens', () => {
    expect(validateName('client-b')).toBe(true);
  });

  it('accepts single char', () => {
    expect(validateName('a')).toBe(true);
  });

  it('accepts digits', () => {
    expect(validateName('team42')).toBe(true);
  });

  it('accepts digit-only', () => {
    expect(validateName('123')).toBe(true);
  });

  it('rejects empty string', () => {
    expect(validateName('')).toBe('Name cannot be empty');
  });

  it('rejects undefined', () => {
    expect(validateName(undefined)).toBe('Name cannot be empty');
  });

  it('rejects uppercase', () => {
    expect(validateName('Acme')).toMatch(/Lowercase/);
  });

  it('rejects spaces', () => {
    expect(validateName('my team')).toMatch(/Lowercase/);
  });

  it('rejects leading hyphen', () => {
    expect(validateName('-acme')).toMatch(/Lowercase/);
  });

  it('rejects trailing hyphen', () => {
    expect(validateName('acme-')).toMatch(/Lowercase/);
  });

  it('rejects underscores', () => {
    expect(validateName('my_team')).toMatch(/Lowercase/);
  });
});

describe('stripUrl', () => {
  it('strips path from valid URL', () => {
    expect(stripUrl('https://acme.atlassian.net/rest/api')).toBe('https://acme.atlassian.net');
  });

  it('handles URL without path', () => {
    expect(stripUrl('https://acme.atlassian.net')).toBe('https://acme.atlassian.net');
  });

  it('handles trailing slash', () => {
    expect(stripUrl('https://acme.atlassian.net/')).toBe('https://acme.atlassian.net');
  });

  it('handles non-URL fallback', () => {
    expect(stripUrl('acme.atlassian.net/foo')).toBe('acme.atlassian.net');
  });
});
