import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { isWindows } from './platform.js';
import { BLUE, GREEN, GRAY, RED, NC } from './colors.js';

// On Windows, Claude Desktop times out MCP server initialize after 60s. The
// `cmd.exe /C npx -y <pkg>` chain Claude uses takes 70-85s cold (registry
// lookup + tarball download + extract + node spawn), so every restart loses
// the race. Pre-warming via `npx` doesn't fully fix it: `npx -y` re-checks
// registry metadata on each invocation, and tagged versions (`@beta`) always
// hit the network. On Windows we therefore install the package globally up
// front and write a direct `node <bin path>` command into the config — spawn
// cost drops to ~300ms and the handshake completes well within the timeout.
//
// macOS/Linux keep the upstream-recommended `npx -y` pattern: cold start fits
// the timeout there (~3-8s, no MSIX env perturbation, no Defender, native
// execve()), and avoiding global installs keeps the user's node_modules
// surface area smaller.

export function parsePackageName(spec) {
  if (spec.startsWith('@')) {
    const slash = spec.indexOf('/');
    if (slash === -1) return spec;
    const at = spec.indexOf('@', slash);
    return at === -1 ? spec : spec.slice(0, at);
  }
  const at = spec.indexOf('@');
  return at === -1 ? spec : spec.slice(0, at);
}

// On Windows `npm` is a .cmd shim. Since Node 20 (CVE-2024-27980) spawning
// .cmd/.bat without `shell: true` fails with EINVAL, so we go through a shell.
// DEP0190 warns about combining shell:true with an args array, but only when
// args contain untrusted input — here every arg is a literal from our own
// integration code, so escaping isn't a concern. We bypass the warning by
// concatenating into a single command string and quoting the package spec.
function spawnNpm(args, opts = {}) {
  const quoted = args.map(a => /[\s"]/.test(a) ? `"${a.replace(/"/g, '\\"')}"` : a).join(' ');
  return spawn(`npm ${quoted}`, { shell: true, ...opts });
}

let cachedNpmRoot = null;
function npmRootGlobal() {
  if (cachedNpmRoot) return Promise.resolve(cachedNpmRoot);
  return new Promise((resolve, reject) => {
    let stdout = '';
    const proc = spawnNpm(['root', '-g']);
    proc.stdout.on('data', d => { stdout += d; });
    proc.on('error', reject);
    proc.on('exit', code => {
      if (code !== 0) return reject(new Error(`'npm root -g' exited with code ${code}`));
      cachedNpmRoot = stdout.trim();
      resolve(cachedNpmRoot);
    });
  });
}

function npmInstallGlobal(spec) {
  return new Promise((resolve, reject) => {
    let stderr = '';
    const proc = spawnNpm(
      ['install', '-g', spec, '--no-audit', '--no-fund', '--no-progress'],
      { stdio: ['ignore', 'ignore', 'pipe'] },
    );
    proc.stderr.on('data', d => { stderr += d; });
    proc.on('error', reject);
    proc.on('exit', code => {
      if (code === 0) return resolve();
      reject(new Error(`'npm install -g ${spec}' failed (exit ${code}): ${stderr.trim().split('\n').slice(-3).join(' ')}`));
    });
  });
}

function resolveBinFromPackageJson(pkgDir, pkgName) {
  const pkgJson = JSON.parse(fs.readFileSync(path.join(pkgDir, 'package.json'), 'utf-8'));
  if (typeof pkgJson.bin === 'string') return pkgJson.bin;
  if (pkgJson.bin && typeof pkgJson.bin === 'object') {
    const entries = Object.entries(pkgJson.bin);
    const shortName = pkgName.replace(/^@[^/]+\//, '');
    const preferred = entries.find(([k]) => k === shortName)
      || entries.find(([k]) => k.startsWith('mcp-'))
      || entries[0];
    return preferred[1];
  }
  throw new Error(`Package ${pkgName} has no 'bin' entry in package.json`);
}

function spinner(message) {
  const isTTY = process.stdout.isTTY;
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  const start = Date.now();
  let i = 0;
  let timer = null;

  const draw = () => {
    const elapsed = Math.floor((Date.now() - start) / 1000);
    process.stdout.write(`\r${BLUE}  ${frames[i]} ${message}${NC} ${GRAY}${elapsed}s${NC}\x1b[K`);
    i = (i + 1) % frames.length;
  };

  if (isTTY) {
    draw();
    timer = setInterval(draw, 100);
  } else {
    process.stdout.write(`  ${message}...\n`);
  }

  return (finalLine) => {
    if (timer) clearInterval(timer);
    if (isTTY) process.stdout.write('\r\x1b[K');
    if (finalLine) console.log(finalLine);
  };
}

export async function installAndResolveBin(spec) {
  const pkgName = parsePackageName(spec);
  const stop = spinner(`Installing ${pkgName}`);
  try {
    await npmInstallGlobal(spec);
  } catch (err) {
    stop(`${RED}  ✗ Failed to install ${pkgName}${NC}`);
    throw err;
  }

  const root = await npmRootGlobal();
  const pkgDir = path.join(root, ...pkgName.split('/'));
  const binRelative = resolveBinFromPackageJson(pkgDir, pkgName);
  const binPath = path.resolve(pkgDir, binRelative);

  if (!fs.existsSync(binPath)) {
    stop(`${RED}  ✗ Installed ${pkgName} but bin file not found${NC}`);
    throw new Error(`bin file not found: ${binPath}`);
  }

  stop(`${GREEN}  ✓ Installed ${pkgName}${NC}`);
  return {
    command: process.execPath,
    args: [binPath],
  };
}

export async function mcpEntry(spec, ...extraArgs) {
  if (!isWindows) {
    return {
      command: 'npx',
      args: ['-y', spec, ...extraArgs],
    };
  }
  const { command, args } = await installAndResolveBin(spec);
  return { command, args: [...args, ...extraArgs] };
}
