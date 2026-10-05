import inquirer from 'inquirer';
import { setServer } from '../config.js';
import { openBrowser } from '../platform.js';
import { stripUrl } from '../validation.js';
import { GREEN, RED, NC } from '../colors.js';
import { resolveInstance } from '../multi-instance.js';
import { offerSecretStorage } from '../op-storage.js';
import { safeFetch } from '../network.js';

export const meta = {
  key: 'jira',
  name: 'Jira',
  configPrefix: 'jira-',
};

export async function setup() {
  const instanceName = await resolveInstance({
    prefix: 'jira-',
    entityLabel: 'instance',
    listMessage: 'Jira instances:',
    formatChoice: (key, val) => `${key} → ${val.env?.JIRA_URL || 'unknown'}`,
  });
  if (!instanceName) return;

  const { jiraUrl } = await inquirer.prompt([{
    type: 'input',
    name: 'jiraUrl',
    message: 'Jira URL (e.g. https://yourcompany.atlassian.net):',
    validate: v => v ? true : 'URL is required',
  }]);

  const { email } = await inquirer.prompt([{
    type: 'input',
    name: 'email',
    message: 'Atlassian account email:',
    validate: v => v ? true : 'Email is required',
  }]);

  console.log('\n  Now you need an API token for your Atlassian account.');
  console.log('  One token works for all Jira sites.\n');
  openBrowser('https://id.atlassian.com/manage-profile/security/api-tokens');

  const { token } = await inquirer.prompt([{
    type: 'password',
    name: 'token',
    message: 'API token:',
    mask: '*',
    validate: v => v ? true : 'Token is required',
  }]);

  const url = stripUrl(jiraUrl);
  console.log('\n  Validating...');

  const resp = await safeFetch(`${url}/rest/api/3/myself`, {
    headers: { Authorization: 'Basic ' + Buffer.from(`${email}:${token}`).toString('base64') },
  });
  if (!resp) return;

  if (!resp.ok) {
    console.log(`${RED}  Authentication failed (HTTP ${resp.status}). Check credentials.${NC}`);
    return;
  }

  const user = await resp.json();
  console.log(`${GREEN}  [ok] Validated (user: ${user.displayName})${NC}\n`);

  const key = `jira-${instanceName}`;
  const stored = await offerSecretStorage(key, token);
  setServer(key, stored.apply({
    command: 'uvx',
    args: ['mcp-atlassian'],
    env: {
      JIRA_URL: url,
      JIRA_USERNAME: email,
      JIRA_API_TOKEN: stored.value,
      TOOLSETS: 'default',
    },
  }));
  console.log(`${GREEN}  [ok] Jira configured (${key})${NC}`);
}
