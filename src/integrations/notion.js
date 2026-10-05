import { resolveInstance } from '../multi-instance.js';
import { safeFetch } from '../network.js';
import { mcpEntry } from '../npm.js';
import { tokenSetupFlow } from '../setup-flow.js';
import { YELLOW, NC } from '../colors.js';

export const meta = {
  key: 'notion',
  name: 'Notion',
  configPrefix: 'notion-',
};

export async function setup() {
  const instanceName = await resolveInstance({
    prefix: 'notion-',
    entityLabel: 'workspace',
    listMessage: 'Notion workspaces:',
    namePrompt: 'Workspace name (e.g. acme, personal):',
    emptyLines: [
      `  ${YELLOW}Notion integrations are workspace-scoped.${NC}`,
      '  Create a separate integration for each workspace you need.\n',
    ],
  });
  if (!instanceName) return;

  const key = `notion-${instanceName}`;

  await tokenSetupFlow({
    configKey: key,
    instructions: [
      '\n  Step 1: Create a Personal Access Token',
      '    1. A browser will open notion.so/developers/tokens',
      '    2. Click "New token"',
      '    3. Select the workspace you want Claude to access',
      '    4. Give it a name (e.g. "Claude MCP")',
      '    5. Copy the token\n',
    ],
    browserUrl: 'https://www.notion.so/developers/tokens',
    tokenPrompt: 'Notion token (starts with ntn_):',
    async validate(token) {
      const resp = await safeFetch('https://api.notion.com/v1/users/me', {
        headers: { Authorization: `Bearer ${token}`, 'Notion-Version': '2022-06-28' },
      });
      if (!resp) return { ok: false, error: 'Network error' };
      if (!resp.ok) return { ok: false, error: `Token invalid (HTTP ${resp.status}). Check and try again.` };
      const data = await resp.json();
      return { ok: true, label: data.name || data.bot?.owner?.user?.name || 'Integration' };
    },
    async buildServerConfig(token) {
      return {
        ...(await mcpEntry('@notionhq/notion-mcp-server')),
        env: { NOTION_TOKEN: token },
      };
    },
  });
}
