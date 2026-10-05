import inquirer from 'inquirer';
import { readConfig } from './config.js';

export async function promptIfConfigured(configKey, actionLabel = 'Refresh token') {
  const config = readConfig();
  if (!config.mcpServers?.[configKey]) return true;

  const { action } = await inquirer.prompt([{
    type: 'list',
    name: 'action',
    message: `${configKey.charAt(0).toUpperCase() + configKey.slice(1)} is already configured. What to do?`,
    choices: [
      { name: actionLabel, value: 'continue' },
      { name: '← Back', value: 'back' },
    ],
  }]);

  return action === 'continue';
}
