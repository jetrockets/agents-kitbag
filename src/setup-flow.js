import inquirer from 'inquirer';
import { setServer } from './config.js';
import { openBrowser } from './platform.js';
import { offerSecretStorage } from './op-storage.js';
import { safeFetch } from './network.js';
import { GREEN, RED, NC } from './colors.js';

export async function tokenSetupFlow({
  configKey,
  instructions,
  browserUrl,
  tokenPrompt = 'API token:',
  validate,
  buildServerConfig,
}) {
  if (instructions) {
    for (const line of instructions) console.log(line);
  }

  if (browserUrl) openBrowser(browserUrl);

  const { token } = await inquirer.prompt([{
    type: 'password',
    name: 'token',
    message: tokenPrompt,
    mask: '*',
    validate: v => v ? true : 'Token is required',
  }]);

  console.log('\n  Validating...');

  const result = await validate(token);
  if (!result.ok) {
    console.log(`${RED}  ${result.error}${NC}`);
    return;
  }

  console.log(`${GREEN}  [ok] Validated (${result.label})${NC}\n`);

  const stored = await offerSecretStorage(configKey, token);
  const serverConfig = await buildServerConfig(stored.value);
  setServer(configKey, stored.apply(serverConfig));
  console.log(`${GREEN}  [ok] ${configKey} configured${NC}`);
}
