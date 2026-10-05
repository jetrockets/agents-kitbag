// Launches an MCP server with secrets fetched from the OS credential store.
//
// Claude Desktop passes an MCP server's `env` through verbatim — no variable
// expansion — so a secret can only reach the server through a wrapper process.
// This is that wrapper, and the reason it exists rather than a shell one-liner
// is Windows, which has no `sh -c`.
//
//   node secret-runner.js --secret VAR=keychain:name [--secret …] -- <cmd> [args…]
//
// stdout belongs to the server's JSON-RPC transport: everything this file
// prints goes to stderr, and nothing ever prints a secret.

import { spawn } from 'node:child_process';
import { backendLabel, isBackendUsable, parseRef, readSecret } from './secret-store.js';

export function parseArgv(argv) {
  const split = argv.indexOf('--');
  if (split === -1) throw new Error('Missing `--` separator before the command to run');

  const secrets = [];
  for (let i = 0; i < split; i++) {
    if (argv[i] !== '--secret') continue;
    const binding = argv[++i];
    const eq = binding?.indexOf('=') ?? -1;
    if (eq < 1) throw new Error(`--secret expects VAR=<backend>:<name>, got "${binding}"`);
    secrets.push({ variable: binding.slice(0, eq), ref: binding.slice(eq + 1) });
  }

  const [command, ...args] = argv.slice(split + 1);
  if (!command) throw new Error('No command given after `--`');
  return { secrets, command, args };
}

export async function resolveEnv(secrets) {
  const env = {};
  for (const { variable, ref } of secrets) {
    const value = await readSecret(ref);
    if (value === null) {
      throw new Error(`Could not read ${variable} from ${ref} — ${await reason(ref)}`);
    }
    env[variable] = value;
  }
  return env;
}

// "the entry is missing" is misleading when the backend command itself is
// absent, which is what happens with a bare system PATH.
async function reason(ref) {
  try {
    const { backend } = parseRef(ref);
    if (!await isBackendUsable(backend)) {
      return `${backendLabel(backend)} is not available here`;
    }
  } catch { /* fall through to the generic reason */ }
  return 'the entry is missing or the store is locked';
}

async function main() {
  let plan;
  try {
    plan = parseArgv(process.argv.slice(2));
  } catch (err) {
    process.stderr.write(`secret-runner: ${err.message}\n`);
    process.exit(64); // EX_USAGE
  }

  let secretEnv;
  try {
    secretEnv = await resolveEnv(plan.secrets);
  } catch (err) {
    process.stderr.write(`secret-runner: ${err.message}\n`);
    process.exit(69); // EX_UNAVAILABLE
  }

  const child = spawn(plan.command, plan.args, {
    stdio: 'inherit',
    env: { ...process.env, ...secretEnv },
  });

  child.on('error', err => {
    process.stderr.write(`secret-runner: cannot start ${plan.command}: ${err.message}\n`);
    process.exit(127);
  });
  // Pass the server's own exit status through, so Claude sees what it would
  // have seen without the wrapper.
  child.on('exit', (code, signal) => process.exit(signal ? 128 : code ?? 0));

  for (const sig of ['SIGINT', 'SIGTERM']) {
    process.on(sig, () => child.kill(sig));
  }
}

// Only run when invoked as a program, so the exports stay testable.
if (process.argv[1] && process.argv[1].endsWith('secret-runner.js')) {
  main();
}
