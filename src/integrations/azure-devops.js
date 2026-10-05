import inquirer from 'inquirer';
import { setServer } from '../config.js';
import { mcpEntry } from '../npm.js';
import { GREEN, NC } from '../colors.js';
import { resolveInstance } from '../multi-instance.js';

export const meta = {
  key: 'azure-devops',
  name: 'Azure DevOps',
  configPrefix: 'ado-',
};

export async function setup() {
  const instanceName = await resolveInstance({
    prefix: 'ado-',
    entityLabel: 'organization',
    listMessage: 'Azure DevOps organizations:',
    formatChoice: (key, val) => {
      const org = val.args?.find((a, i) => i > 0 && !a.startsWith('-')) || 'unknown';
      return `${key} → ${org}`;
    },
  });
  if (!instanceName) return;

  let orgName;

  const { input } = await inquirer.prompt([{
    type: 'input',
    name: 'input',
    message: 'Azure DevOps URL or organization name (e.g. https://dev.azure.com/Contoso/... or just Contoso):',
    validate: v => v ? true : 'Required',
  }]);

  const urlMatch = input.trim().match(/dev\.azure\.com\/([^/]+)/);
  orgName = urlMatch ? urlMatch[1] : input.trim();

  // https://github.com/microsoft/azure-devops-mcp
  const key = `ado-${instanceName}`;
  setServer(key, await mcpEntry('@azure-devops/mcp', orgName));
  console.log(`${GREEN}  [ok] Azure DevOps configured (${key} → ${orgName})${NC}`);
  console.log('\n  On first use, a browser will open for Microsoft account login.');
}
