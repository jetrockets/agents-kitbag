import { RED, NC } from './colors.js';

const DEFAULT_TIMEOUT_MS = 5000;

export async function safeFetch(url, options = {}) {
  const { timeout = DEFAULT_TIMEOUT_MS, ...fetchOptions } = options;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeout);
  try {
    return await fetch(url, { ...fetchOptions, signal: controller.signal });
  } catch (err) {
    const message = err.name === 'AbortError'
      ? `Network timeout after ${timeout}ms`
      : err.code === 'ENOTFOUND'
        ? 'DNS lookup failed — check your internet connection'
        : err.code === 'ECONNREFUSED'
          ? 'Connection refused — server may be down'
          : `Network error: ${err.message}`;
    console.log(`${RED}  ${message}${NC}`);
    return null;
  } finally {
    clearTimeout(timer);
  }
}
