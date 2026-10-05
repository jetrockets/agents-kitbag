import inquirer from 'inquirer';
import { readConfig } from './config.js';
import { validateName } from './validation.js';

export async function resolveInstance({
  prefix,
  entityLabel = 'instance',
  listMessage,
  formatChoice,
  emptyLines = [],
  namePrompt,
}) {
  const config = readConfig();
  const servers = config.mcpServers || {};
  const instances = Object.entries(servers).filter(([k]) => k.startsWith(prefix));

  const promptMsg = namePrompt || `Instance name (e.g. acme, client-b):`;

  if (instances.length > 0) {
    const choices = instances.map(([key, val]) => ({
      name: formatChoice ? formatChoice(key, val) : key,
      value: key,
    }));
    choices.push({ name: `+ Add new ${entityLabel}`, value: '__new__' });
    choices.push({ name: '← Back', value: '__back__' });

    const { action } = await inquirer.prompt([{
      type: 'list',
      name: 'action',
      message: listMessage,
      choices,
    }]);

    if (action === '__back__') return null;

    if (action === '__new__') {
      const { name } = await inquirer.prompt([{
        type: 'input',
        name: 'name',
        message: promptMsg,
        validate: validateName,
      }]);
      return name;
    }

    return action.replace(prefix, '');
  }

  console.log(`\n  No ${listMessage.replace(':', '')} configured yet.\n`);
  for (const line of emptyLines) console.log(line);

  const { name } = await inquirer.prompt([{
    type: 'input',
    name: 'name',
    message: promptMsg,
    validate: validateName,
  }]);
  return name;
}
