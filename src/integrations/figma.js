import { promptIfConfigured } from '../single-instance.js';
import { safeFetch } from '../network.js';
import { mcpEntry } from '../npm.js';
import { tokenSetupFlow } from '../setup-flow.js';

export const meta = {
  key: 'figma',
  name: 'Figma',
  configKey: 'figma',
};

export async function setup() {
  if (!await promptIfConfigured('figma', 'Refresh token')) return;

  await tokenSetupFlow({
    configKey: 'figma',
    instructions: [
      '\n  Step 1: Create personal access token',
      '    1. A browser will open Figma',
      '    2. Open Settings (avatar top-left → Settings) — do everything there:',
      '       - On "Account" tab, scroll to "Personal access tokens"',
      '       - Click "Generate new token", name it (e.g. "Claude MCP")',
      '       - Select required scopes (recommended: File content — Read,',
      '         File metadata — Read, Comments — Read, Dev resources — Read)',
      '       - Copy the token (starts with figd_)\n',
    ],
    browserUrl: 'https://www.figma.com/files',
    tokenPrompt: 'Figma token (starts with figd_):',
    async validate(token) {
      const resp = await safeFetch('https://api.figma.com/v1/me', {
        headers: { 'X-Figma-Token': token },
      });
      if (!resp) return { ok: false, error: 'Network error' };
      if (!resp.ok) return { ok: false, error: `Token invalid (HTTP ${resp.status}). Check and try again.` };
      const data = await resp.json();
      return { ok: true, label: data.handle || data.email || 'Account' };
    },
    async buildServerConfig(token) {
      return {
        ...(await mcpEntry('figma-developer-mcp', '--stdio')),
        env: { FIGMA_API_KEY: token },
      };
    },
  });
}
