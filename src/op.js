import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { isWindows } from './platform.js';

// Secrets live in 1Password and the config file holds only `op://` references.
// Claude Desktop does not expand environment variables in an MCP server's `env`,
// so the reference has to be resolved by a wrapper process: we rewrite the
// server's command to `op run -- <original command>`, and op substitutes every
// op:// value in `env` before exec'ing the real server.

const SECRET_REF = /^op:\/\//;

// Claude Desktop launched from the Dock inherits a bare system PATH, so a bare
// "op" in the config would fail to spawn. We probe the standard install
// locations and write an absolute path.
// Only ever .exe on Windows: since Node 20 (CVE-2024-27980) spawning a .cmd or
// .bat without shell:true fails with EINVAL, and Claude Desktop spawns the
// configured command directly. Scoop and Chocolatey both ship an .exe shim
// alongside their .cmd one.
// path.win32 rather than path: the separator has to be a backslash regardless of
// the platform building the list, so this stays testable off Windows.
const win = path.win32.join;
const CANDIDATES = isWindows
  ? [
      win(process.env.LOCALAPPDATA || '', 'Microsoft', 'WinGet', 'Links', 'op.exe'),
      win(process.env.ProgramFiles || '', '1Password CLI', 'op.exe'),
      win(process.env['ProgramFiles(x86)'] || '', '1Password CLI', 'op.exe'),
      win(process.env.USERPROFILE || '', 'scoop', 'shims', 'op.exe'),
      win(process.env.ProgramData || '', 'chocolatey', 'bin', 'op.exe'),
    ]
  : [
      '/opt/homebrew/bin/op',
      '/usr/local/bin/op',
      '/usr/bin/op',
      path.join(process.env.HOME || '', '.local', 'bin', 'op'),
    ];

export function resolveOpPath() {
  for (const candidate of CANDIDATES) {
    if (candidate && fs.existsSync(candidate)) return candidate;
  }
  return null;
}

export function isSecretRef(value) {
  return typeof value === 'string' && SECRET_REF.test(value);
}

export function secretRef(vault, title, field = 'credential') {
  return `op://${vault}/${title}/${field}`;
}

export function itemTitle(configKey) {
  return `Claude MCP - ${configKey}`;
}

// Assignment statements land in argv, where any process on the machine can read
// them off the process list. A JSON template does not.
export function itemTemplate(title, token) {
  return JSON.stringify({
    title,
    category: 'API_CREDENTIAL',
    fields: [
      { id: 'credential', type: 'CONCEALED', label: 'credential', value: token },
      { id: 'notesPlain', type: 'STRING', purpose: 'NOTES', label: 'notesPlain', value: 'Created by claude-toolkit.' },
    ],
  });
}

export function isOpWrapped(serverConfig) {
  const command = serverConfig?.command;
  if (typeof command !== 'string') return false;
  const bin = command.split(/[\\/]/).pop().toLowerCase();
  return (bin === 'op' || bin === 'op.exe') && serverConfig.args?.[0] === 'run';
}

// `op run` conceals secrets found on stdout by default. stdout is the MCP
// server's JSON-RPC transport, so a token echoed back inside a response would be
// rewritten mid-payload — hence --no-masking.
export function wrapWithOpRun(serverConfig, opBinary) {
  if (isOpWrapped(serverConfig)) return serverConfig;
  const { command, args = [], ...rest } = serverConfig;
  return { ...rest, command: opBinary, args: ['run', '--no-masking', '--', command, ...args] };
}

function run(opBinary, args, { input } = {}) {
  return new Promise(resolve => {
    const proc = spawn(opBinary, args, { stdio: ['pipe', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    proc.stdout.on('data', d => { stdout += d; });
    proc.stderr.on('data', d => { stderr += d; });
    proc.on('error', err => resolve({ code: -1, stdout: '', stderr: err.message }));
    proc.on('close', code => resolve({ code, stdout, stderr }));
    if (input !== undefined) proc.stdin.end(input);
    else proc.stdin.end();
  });
}

// Not `op whoami`: with the 1Password desktop app integration (rather than a
// session created by `op signin`) whoami reports "account is not signed in"
// while reads succeed through biometric unlock. Listing vaults is the cheapest
// call that actually proves the CLI can reach the user's data.
export async function isUsable(opBinary = resolveOpPath()) {
  if (!opBinary) return false;
  const { code } = await run(opBinary, ['vault', 'list', '--format=json']);
  return code === 0;
}

export async function listVaults(opBinary = resolveOpPath()) {
  if (!opBinary) return [];
  const { code, stdout } = await run(opBinary, ['vault', 'list', '--format=json']);
  if (code !== 0) return [];
  try {
    return JSON.parse(stdout).map(v => v.name);
  } catch {
    return [];
  }
}

// Returns the op:// reference to the stored token. Throws with op's own stderr
// so the caller can show the user what 1Password objected to.
export async function createItem({ title, vault, token, opBinary = resolveOpPath() }) {
  // `op item create -` reads its template only from a real pipe. Node's stdio
  // pipes are socketpairs, which op silently ignores: it creates an "Untitled"
  // item with none of the template applied. --template takes a file instead, so
  // the token touches disk briefly — owner-only, in a private temp directory,
  // removed immediately — rather than sitting in argv for every process to read.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'claude-toolkit-'));
  const templatePath = path.join(dir, 'item.json');
  let result;
  try {
    fs.writeFileSync(templatePath, itemTemplate(title, token), { mode: 0o600 });
    result = await run(
      opBinary,
      ['item', 'create', `--template=${templatePath}`, '--vault', vault, '--format=json'],
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
  const { code, stdout, stderr } = result;
  if (code !== 0) throw new Error(stderr.trim() || `op item create exited with code ${code}`);

  // Reference the item by title rather than by the id op just returned: a title
  // stays readable in the config file, and `op read` accepts either.
  let storedTitle = title;
  try {
    storedTitle = JSON.parse(stdout).title || title;
  } catch { /* fall back to the requested title */ }
  return secretRef(vault, storedTitle);
}

export async function readSecret(ref, opBinary = resolveOpPath()) {
  if (!opBinary || !isSecretRef(ref)) return null;
  const { code, stdout } = await run(opBinary, ['read', ref]);
  return code === 0 ? stdout.trim() : null;
}
