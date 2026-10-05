import inquirer from 'inquirer';
import { setServer } from '../config.js';
import { openBrowser } from '../platform.js';
import { mcpEntry } from '../npm.js';
import { GREEN, RED, NC } from '../colors.js';
import { resolveInstance } from '../multi-instance.js';
import { offerSecretStorage } from '../op-storage.js';
import { safeFetch } from '../network.js';

export const meta = {
  key: 'linear',
  name: 'Linear',
  configPrefix: 'linear-',
};

export async function setup() {
  const instanceName = await resolveInstance({
    prefix: 'linear-',
    entityLabel: 'instance',
    listMessage: 'Linear instances:',
    emptyLines: [
      '  IMPORTANT: API keys are workspace-scoped!',
      '  Make sure you are in the correct workspace before creating the key.\n',
    ],
  });
  if (!instanceName) return;

  console.log('\n  1. Open Linear and switch to the correct workspace (top-left corner)');
  console.log('  2. Go to Settings > Security & access > Personal API Keys');
  console.log('  3. Click "Create key" and paste below\n');
  openBrowser('https://linear.app/settings/account/security');

  const { token } = await inquirer.prompt([{
    type: 'password',
    name: 'token',
    message: 'API key:',
    mask: '*',
    validate: v => v ? true : 'API key is required',
  }]);

  // Validate
  console.log('\n  Validating...');

  const resp = await safeFetch('https://api.linear.app/graphql', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: token,
    },
    body: JSON.stringify({ query: '{ viewer { id name email } }' }),
  });
  if (!resp) return;

  const data = await resp.json();
  const viewer = data?.data?.viewer;

  if (!viewer?.name) {
    const err = data?.errors?.[0]?.message || 'unknown error';
    console.log(`${RED}  Authentication failed: ${err}${NC}`);
    return;
  }

  console.log(`${GREEN}  [ok] Validated (user: ${viewer.name}, ${viewer.email})${NC}\n`);

  const key = `linear-${instanceName}`;
  const stored = await offerSecretStorage(key, token, { envVar: 'LINEAR_MCP_TOKEN' });
  setServer(key, stored.apply({
    ...(await mcpEntry('mcp-remote', 'https://mcp.linear.app/mcp', '--header', `Authorization: Bearer ${stored.value}`)),
    env: {},
  }));
  console.log(`${GREEN}  [ok] Linear configured (${key})${NC}`);
}
