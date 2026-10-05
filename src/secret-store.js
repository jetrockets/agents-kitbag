import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

// Secrets kept outside claude_desktop_config.json, for people without 1Password.
// Claude Desktop does not expand variables in an MCP server's `env`, so the
// config holds a `<backend>:<name>` reference and secret-runner.js resolves it
// at launch. Backends are whatever the OS already ships with.

const BACKENDS = ['keychain', 'dpapi', 'libsecret', 'gh'];

const LABELS = {
  keychain: 'macOS Keychain',
  dpapi: 'Windows Credential store (DPAPI)',
  libsecret: 'Secret Service (libsecret)',
  gh: 'GitHub CLI (gh auth token)',
};

export function backendLabel(backend) {
  return LABELS[backend] || backend;
}

export function defaultBackend() {
  if (process.platform === 'darwin') return 'keychain';
  if (process.platform === 'win32') return 'dpapi';
  return 'libsecret';
}

export function parseRef(ref) {
  const at = typeof ref === 'string' ? ref.indexOf(':') : -1;
  if (at === -1) throw new Error(`Secret reference needs a backend prefix, got "${ref}"`);
  const backend = ref.slice(0, at);
  if (!BACKENDS.includes(backend)) throw new Error(`Unknown backend "${backend}"`);
  return { backend, name: ref.slice(at + 1) };
}

export function isSecretRef(value) {
  if (typeof value !== 'string') return false;
  try {
    parseRef(value);
    return true;
  } catch {
    return false;
  }
}

export function secretRefFor(backend, configKey) {
  // The GitHub CLI is the store; there is nothing of ours to name.
  return backend === 'gh' ? 'gh:' : `${backend}:claude-mcp-${configKey}`;
}

// Claude Desktop launched from the Dock inherits a bare system PATH, so `gh`
// and `secret-tool` would not resolve by name and the server would fail to
// start. Probe the usual install locations and call the backend by absolute
// path, falling back to the bare name for an install we do not know about.
const win = path.win32.join;
const CANDIDATES = {
  gh: [
    '/opt/homebrew/bin/gh',
    '/usr/local/bin/gh',
    '/usr/bin/gh',
    win(process.env.ProgramFiles || '', 'GitHub CLI', 'gh.exe'),
    win(process.env.LOCALAPPDATA || '', 'Microsoft', 'WinGet', 'Links', 'gh.exe'),
  ],
  security: ['/usr/bin/security'],
  'secret-tool': ['/usr/bin/secret-tool', '/usr/local/bin/secret-tool', '/opt/homebrew/bin/secret-tool'],
  powershell: [
    win(process.env.SystemRoot || '', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'),
  ],
};

const resolved = new Map();

export function resolveBin(name) {
  if (resolved.has(name)) return resolved.get(name);
  const found = (CANDIDATES[name] || []).find(c => c && fs.existsSync(c)) || name;
  resolved.set(name, found);
  return found;
}

function run(cmd, args, { input } = {}) {
  return new Promise(resolve => {
    const proc = spawn(cmd, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', d => { stdout += d; });
    proc.stderr.on('data', d => { stderr += d; });
    proc.on('error', err => resolve({ code: -1, stdout: '', stderr: err.message }));
    proc.on('close', code => resolve({ code, stdout, stderr }));
    proc.stdin.end(input ?? '');
  });
}

// PowerShell one-liners for the Windows backend. DPAPI encrypts to the current
// user on the current machine, so the ciphertext file is useless anywhere else.
// The plaintext travels through stdin, never as a command argument.
const psRead = name => `
$ErrorActionPreference = 'Stop'
$file = Join-Path $env:APPDATA 'claude-toolkit\\secrets\\${name}.dpapi'
if (-not (Test-Path $file)) { exit 1 }
$secure = Get-Content $file | ConvertTo-SecureString
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try { [Console]::Out.Write([Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)) }
finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
`;

const psWrite = name => `
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:APPDATA 'claude-toolkit\\secrets'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$plain = [Console]::In.ReadToEnd()
$secure = ConvertTo-SecureString $plain -AsPlainText -Force
ConvertFrom-SecureString $secure | Set-Content -Path (Join-Path $dir '${name}.dpapi') -NoNewline
`;

// Returns the secret, or null when the entry is missing or the store is locked.
export async function readSecret(ref) {
  const { backend, name } = parseRef(ref);

  const { code, stdout } = await (() => {
    switch (backend) {
      case 'keychain':
        return run(resolveBin('security'), ['find-generic-password', '-a', process.env.USER || '', '-s', name, '-w']);
      case 'libsecret':
        return run(resolveBin('secret-tool'), ['lookup', 'service', name]);
      case 'dpapi':
        return run(resolveBin('powershell'), ['-NoProfile', '-NonInteractive', '-Command', psRead(name)]);
      case 'gh':
        return run(resolveBin('gh'), ['auth', 'token']);
      default:
        return Promise.resolve({ code: 1, stdout: '' });
    }
  })();

  return code === 0 && stdout ? stdout.replace(/\r?\n$/, '') : null;
}

// Throws with the backend's own message so the caller can show what went wrong.
export async function storeSecret(ref, secret) {
  const { backend, name } = parseRef(ref);
  if (backend === 'gh') return; // gh owns its token; nothing for us to write

  const { code, stderr } = await (() => {
    switch (backend) {
      case 'keychain':
        // `-w` last makes security prompt for the value and its confirmation, so
        // both come from stdin and the secret stays out of argv.
        return run(resolveBin('security'), ['add-generic-password', '-a', process.env.USER || '', '-s', name, '-U', '-w'],
          { input: `${secret}\n${secret}\n` });
      case 'libsecret':
        return run(resolveBin('secret-tool'), ['store', '--label', name, 'service', name], { input: secret });
      case 'dpapi':
        return run(resolveBin('powershell'), ['-NoProfile', '-NonInteractive', '-Command', psWrite(name)], { input: secret });
      default:
        return Promise.resolve({ code: 1, stderr: `Unknown backend "${backend}"` });
    }
  })();

  if (code !== 0) throw new Error(stderr.trim() || `${backend} store failed with code ${code}`);
}

export async function isBackendUsable(backend) {
  switch (backend) {
    case 'keychain':
      return (await run(resolveBin('security'), ['error', '0'])).code === 0;
    case 'libsecret':
      return (await run(resolveBin('secret-tool'), ['--version'])).code === 0;
    case 'dpapi':
      return (await run(resolveBin('powershell'), ['-NoProfile', '-NonInteractive', '-Command', '$PSVersionTable.PSVersion.Major'])).code === 0;
    case 'gh':
      return (await run(resolveBin('gh'), ['auth', 'token'])).code === 0;
    default:
      return false;
  }
}
