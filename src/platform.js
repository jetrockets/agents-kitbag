import { exec } from 'node:child_process';

export const isWindows = process.platform === 'win32';

export function openBrowser(url) {
  const cmd = isWindows
    ? `start "" "${url}"`
    : process.platform === 'linux'
      ? `xdg-open "${url}"`
      : `open "${url}"`;
  exec(cmd, () => {});
}
