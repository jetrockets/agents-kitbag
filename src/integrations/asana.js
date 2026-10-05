import { promptIfConfigured } from '../single-instance.js';
import { safeFetch } from '../network.js';
import { mcpEntry } from '../npm.js';
import { tokenSetupFlow } from '../setup-flow.js';

export const meta = {
  key: 'asana',
  name: 'Asana',
  configKey: 'asana',
};

export async function setup() {
  if (!await promptIfConfigured('asana', 'Update token')) return;

  await tokenSetupFlow({
    configKey: 'asana',
    instructions: [
      '\n  One token gives access to all your Asana workspaces.',
      '  1. Open Asana → My Settings → Apps → Personal access tokens',
      '  2. Click "Create new token" and paste below\n',
    ],
    browserUrl: 'https://app.asana.com/0/my-apps',
    tokenPrompt: 'Asana token:',
    async validate(token) {
      const resp = await safeFetch('https://app.asana.com/api/1.0/users/me', {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!resp) return { ok: false, error: 'Network error' };
      if (!resp.ok) return { ok: false, error: `Token invalid (HTTP ${resp.status}). Check and try again.` };
      const data = await resp.json();
      return { ok: true, label: `user: ${data.data.name}` };
    },
    async buildServerConfig(token) {
      return {
        ...(await mcpEntry('@roychri/mcp-server-asana@beta')),
        env: { ASANA_ACCESS_TOKEN: token },
      };
    },
  });
}
