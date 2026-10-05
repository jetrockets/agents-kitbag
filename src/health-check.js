import { readConfig } from './config.js';
import { safeFetch } from './network.js';
import { isOpWrapped, isSecretRef, isUsable, readSecret } from './op.js';
import { readSecret as readFromStore } from './secret-store.js';
import { GREEN, RED, YELLOW, GRAY, NC } from './colors.js';

function extractBearerToken(serverConfig) {
  const args = serverConfig.args || [];
  const headerIdx = args.indexOf('--header');
  if (headerIdx === -1 || headerIdx + 1 >= args.length) return null;
  const headerVal = args[headerIdx + 1];
  const match = headerVal.match(/^Authorization:\s*Bearer\s+(.+)$/i);
  return match ? match[1] : null;
}

// Servers launched through secret-runner.js carry their secret as a
// `--secret VAR=<backend>:<name>` binding rather than anywhere in env.
function runnerBinding(serverConfig, variable) {
  const args = serverConfig?.args || [];
  if (!args.some(a => typeof a === 'string' && a.endsWith('secret-runner.js'))) return null;
  const stop = args.indexOf('--');
  for (let i = 0; i < (stop === -1 ? args.length : stop); i++) {
    if (args[i] !== '--secret') continue;
    const [name, ...rest] = (args[i + 1] || '').split('=');
    if (!variable || name === variable) return rest.join('=') || null;
  }
  return null;
}

// A configured token may be the literal value, an op:// reference, a ${VAR}
// placeholder pointing at one in env (mcp-remote expands those in headers at
// launch), or a binding on the runner's command line. Health checks need the
// real value behind all four.
async function resolveSecret(serverConfig, value) {
  const placeholder = typeof value === 'string' ? value.match(/^\$\{([^}]+)\}$/) : null;
  if (placeholder) {
    const variable = placeholder[1];
    const binding = runnerBinding(serverConfig, variable);
    if (binding) return await readFromStore(binding);
    value = serverConfig.env?.[variable];
  }

  if (!value) {
    const binding = runnerBinding(serverConfig, null);
    return binding ? await readFromStore(binding) : null;
  }

  return isSecretRef(value) ? await readSecret(value) : value;
}

function classifyResponse(resp) {
  if (!resp) return { status: 'error', detail: 'network error' };
  if (resp.ok) return { status: 'ok', detail: null };
  if (resp.status === 401 || resp.status === 403) return { status: 'expired', detail: `HTTP ${resp.status}` };
  return { status: 'error', detail: `HTTP ${resp.status}` };
}

async function checkGitHub(serverConfig) {
  const token = await resolveSecret(serverConfig, extractBearerToken(serverConfig));
  if (!token) return { status: 'error', detail: 'token missing from args' };
  const resp = await safeFetch('https://api.github.com/user', {
    headers: { Authorization: `Bearer ${token}`, 'User-Agent': 'claude-toolkit' },
  });
  return classifyResponse(resp);
}

async function checkAsana(serverConfig) {
  const token = await resolveSecret(serverConfig, serverConfig.env?.ASANA_ACCESS_TOKEN);
  if (!token) return { status: 'error', detail: 'token missing' };
  const resp = await safeFetch('https://app.asana.com/api/1.0/users/me', {
    headers: { Authorization: `Bearer ${token}` },
  });
  return classifyResponse(resp);
}

async function checkFigma(serverConfig) {
  const token = await resolveSecret(serverConfig, serverConfig.env?.FIGMA_API_KEY);
  if (!token) return { status: 'error', detail: 'token missing' };
  const resp = await safeFetch('https://api.figma.com/v1/me', {
    headers: { 'X-Figma-Token': token },
  });
  return classifyResponse(resp);
}

async function checkJira(serverConfig) {
  const url = serverConfig.env?.JIRA_URL;
  const username = serverConfig.env?.JIRA_USERNAME;
  const token = await resolveSecret(serverConfig, serverConfig.env?.JIRA_API_TOKEN);
  if (!url || !username || !token) return { status: 'error', detail: 'credentials missing' };
  const resp = await safeFetch(`${url}/rest/api/3/myself`, {
    headers: { Authorization: 'Basic ' + Buffer.from(`${username}:${token}`).toString('base64') },
  });
  return classifyResponse(resp);
}

async function checkNotion(serverConfig) {
  const token = await resolveSecret(serverConfig, serverConfig.env?.NOTION_TOKEN);
  if (!token) return { status: 'error', detail: 'token missing' };
  const resp = await safeFetch('https://api.notion.com/v1/users/me', {
    headers: { Authorization: `Bearer ${token}`, 'Notion-Version': '2022-06-28' },
  });
  return classifyResponse(resp);
}

async function checkLinear(serverConfig) {
  const token = await resolveSecret(serverConfig, extractBearerToken(serverConfig));
  if (!token) return { status: 'error', detail: 'token missing from args' };
  const resp = await safeFetch('https://api.linear.app/graphql', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: token },
    body: JSON.stringify({ query: '{ viewer { id } }' }),
  });
  if (!resp) return { status: 'error', detail: 'network error' };
  if (!resp.ok) return classifyResponse(resp);
  const data = await resp.json();
  if (!data?.data?.viewer?.id) return { status: 'expired', detail: 'invalid token' };
  return { status: 'ok', detail: null };
}

const prefixValidators = {
  'jira-': checkJira,
  'linear-': checkLinear,
  'notion-': checkNotion,
  'ado-': () => ({ status: 'skip', detail: 'browser auth' }),
};

const exactValidators = {
  github: checkGitHub,
  asana: checkAsana,
  figma: checkFigma,
};

export async function checkAllTokens() {
  const config = readConfig();
  const servers = config.mcpServers || {};
  const keys = Object.keys(servers);
  if (keys.length === 0) return [];

  // Every op-backed server would otherwise report "token missing" when
  // 1Password is locked, which sends the user off re-running setup for nothing.
  const opBacked = keys.some(key => isOpWrapped(servers[key]));
  const opUsable = opBacked ? await isUsable() : false;

  const tasks = keys.map(async (key) => {
    if (isOpWrapped(servers[key]) && !opUsable) {
      return { key, status: 'skip', detail: '1Password locked — unlock to check' };
    }
    // A runner-backed server whose secret cannot be read will not start at all.
    const binding = runnerBinding(servers[key], null);
    if (binding && await readFromStore(binding) === null) {
      return { key, status: 'skip', detail: `credential store has no ${binding} — re-run setup` };
    }
    if (exactValidators[key]) {
      const result = await exactValidators[key](servers[key]);
      return { key, ...result };
    }
    for (const [prefix, validator] of Object.entries(prefixValidators)) {
      if (key.startsWith(prefix)) {
        const result = await validator(servers[key]);
        return { key, ...result };
      }
    }
    return { key, status: 'skip', detail: 'unknown integration' };
  });

  const results = await Promise.allSettled(tasks);
  return results.map(r => r.status === 'fulfilled'
    ? r.value
    : { key: 'unknown', status: 'error', detail: r.reason?.message || 'unexpected error' },
  );
}

export function printHealthResults(results) {
  if (results.length === 0) return;

  const configured = results.filter(r => r.status !== 'skip' || r.detail === 'browser auth');
  if (configured.length === 0) return;

  console.log(`\n  ${GRAY}Token health check:${NC}`);
  for (const r of results) {
    const label = r.key.padEnd(16);
    switch (r.status) {
      case 'ok':
        console.log(`  ${GREEN}  ✓ ${label}${NC}`);
        break;
      case 'expired':
        console.log(`  ${RED}  ✗ ${label}${NC} ${YELLOW}token expired — re-run setup${NC}`);
        break;
      case 'error':
        console.log(`  ${RED}  ✗ ${label}${NC} ${GRAY}${r.detail}${NC}`);
        break;
      case 'skip':
        console.log(`  ${GRAY}  ⏭ ${label}${r.detail}${NC}`);
        break;
    }
  }
  console.log('');
}

export { extractBearerToken, classifyResponse, resolveSecret };
