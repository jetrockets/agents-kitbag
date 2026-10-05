import path from 'node:path';
import { fileURLToPath } from 'node:url';
import inquirer from 'inquirer';
import { createItem, isUsable, itemTitle, listVaults, resolveOpPath, wrapWithOpRun } from './op.js';
import { backendLabel, defaultBackend, isBackendUsable, secretRefFor, storeSecret } from './secret-store.js';
import { GRAY, GREEN, YELLOW, NC } from './colors.js';

const runnerPath = path.join(path.dirname(fileURLToPath(import.meta.url)), 'secret-runner.js');

// Marks where a token would have gone in an integration's `env`, so `apply` can
// find the variable name and move it into a --secret binding instead.
const PLACEHOLDER = '__CLAUDE_TOOLKIT_SECRET__';

const plaintext = token => ({ value: token, apply: config => config });

// Offers to keep the token out of claude_desktop_config.json. Returns the value
// the integration should build its server config with, plus an `apply` that
// rewrites the finished config to fetch the secret at launch.
//
// `envVar` is for integrations that carry the token in `args` rather than `env`
// (GitHub and Linear pass it as an Authorization header): the placeholder goes
// into the header and the variable is supplied by the wrapper at startup.
export async function offerSecretStorage(configKey, token, { envVar, ghSource = false } = {}) {
  const choices = [];

  // For GitHub the CLI already holds the token in the OS keychain, so the best
  // option is not to keep a second copy at all — the wrapper asks gh at launch.
  if (ghSource && await isBackendUsable('gh')) {
    choices.push({ name: backendLabel('gh') + ' — no second copy stored', value: 'gh' });
  }

  const opBinary = resolveOpPath();
  const opReady = opBinary ? await isUsable(opBinary) : false;
  if (opReady) choices.push({ name: '1Password', value: 'op' });

  const backend = defaultBackend();
  const backendReady = await isBackendUsable(backend);
  if (backendReady) choices.push({ name: backendLabel(backend), value: 'system' });

  if (choices.length === 0) {
    console.log(`${GRAY}  No credential store available — storing the token in the config file.${NC}`);
    if (opBinary && !opReady) {
      console.log(`${GRAY}  For 1Password: Settings → Developer → Integrate with 1Password CLI.${NC}`);
    }
    return plaintext(token);
  }

  choices.push({ name: 'Config file (plain text)', value: 'plain' });

  const { store } = await inquirer.prompt([{
    type: 'list',
    name: 'store',
    message: 'Where should this token be kept?',
    choices,
  }]);

  if (store === 'plain') return plaintext(token);
  if (store === 'op') return storeInOnePassword({ configKey, token, envVar, opBinary });
  if (store === 'gh') return useRunner({ ref: 'gh:', envVar, label: backendLabel('gh') });
  return storeInSystemStore({ configKey, token, envVar, backend });
}

async function storeInOnePassword({ configKey, token, envVar, opBinary }) {
  const vault = await chooseVault();
  let reference;
  try {
    reference = await createItem({ title: itemTitle(configKey), vault, token, opBinary });
  } catch (err) {
    console.log(`${YELLOW}  Could not save to 1Password (${err.message}) — storing the token in the config file.${NC}`);
    return plaintext(token);
  }

  console.log(`${GREEN}  [ok] Saved to 1Password: ${reference}${NC}`);

  return {
    value: envVar ? `\${${envVar}}` : reference,
    apply(config) {
      const withRef = envVar
        ? { ...config, env: { ...config.env, [envVar]: reference } }
        : config;
      return wrapWithOpRun(withRef, opBinary);
    },
  };
}

async function storeInSystemStore({ configKey, token, envVar, backend }) {
  const ref = secretRefFor(backend, configKey);
  try {
    await storeSecret(ref, token);
  } catch (err) {
    console.log(`${YELLOW}  Could not save to ${backendLabel(backend)} (${err.message}) — storing the token in the config file.${NC}`);
    return plaintext(token);
  }

  console.log(`${GREEN}  [ok] Saved to ${backendLabel(backend)}: ${ref}${NC}`);
  return useRunner({ ref, envVar });
}

// Builds a config that launches the server through secret-runner.js, which
// resolves `ref` at startup.
function useRunner({ ref, envVar, label }) {
  if (label) console.log(`${GREEN}  [ok] Token will be read from ${label} at launch${NC}`);

  return {
    value: envVar ? `\${${envVar}}` : PLACEHOLDER,
    apply(config) {
      const { command, args = [], env = {}, ...rest } = config;

      // In env mode the integration put the placeholder under its own variable
      // name; the wrapper supplies that variable instead, so drop it here.
      const variables = envVar
        ? [envVar]
        : Object.keys(env).filter(k => env[k] === PLACEHOLDER);
      const remainingEnv = Object.fromEntries(
        Object.entries(env).filter(([, v]) => v !== PLACEHOLDER),
      );

      const bindings = variables.flatMap(v => ['--secret', `${v}=${ref}`]);
      return {
        ...rest,
        command: process.execPath,
        args: [runnerPath, ...bindings, '--', command, ...args],
        env: remainingEnv,
      };
    },
  };
}

async function chooseVault() {
  const vaults = await listVaults();
  if (vaults.length <= 1) return vaults[0] || 'Private';

  const { vault } = await inquirer.prompt([{
    type: 'list',
    name: 'vault',
    message: 'Which 1Password vault?',
    choices: vaults,
    default: vaults.includes('Private') ? 'Private' : vaults[0],
  }]);
  return vault;
}
