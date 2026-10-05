import inquirer from 'inquirer';
import { ExitPromptError } from '@inquirer/core';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { configPath, getStatus, deleteByMeta } from './config.js';
import { purgeBackups } from './backup.js';
import { restartClaude } from './claude.js';
import { BLUE, BOLD, CYAN, GRAY, GREEN, RED, YELLOW, NC } from './colors.js';
import { checkAllTokens, printHealthResults } from './health-check.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Brief countdown before the menu redraws. We tried four versions of a
// manual "Press Enter to continue" prompt — inquirer-based, raw stdin,
// readline.createInterface, removeAllListeners + filter — and each one had
// its own way of fighting inquirer's stdin state, ending in either the
// user needing multiple Enter presses, or the next menu's arrow keys
// printing as "^[[A" because inquirer couldn't re-enter raw mode. An
// automatic countdown sidesteps the entire problem, and the post-setup
// "Next step" block stays on screen long enough to actually be read.
async function waitForEnter() {
  const seconds = 3;
  for (let i = seconds; i > 0; i--) {
    process.stdout.write(`\r${CYAN}  Returning to menu in ${i}…${NC}\x1b[K`);
    await new Promise(r => setTimeout(r, 1000));
  }
  process.stdout.write('\r\x1b[K');
}

// Auto-discover integrations from src/integrations/*.js
// Each module must export: meta { key, name, configPrefix? | configKey? } and setup()
async function loadIntegrations() {
  const dir = path.join(__dirname, 'integrations');
  const files = fs.readdirSync(dir).filter(f => f.endsWith('.js') && f !== 'skills.js');
  const integrations = [];

  for (const file of files) {
    const mod = await import(`./integrations/${file}`);
    if (mod.meta && mod.setup) {
      integrations.push({ ...mod.meta, setup: mod.setup });
    }
  }

  return integrations;
}

function showBanner() {
  console.clear();
  console.log('');
  console.log(`${BLUE}${BOLD}   ██████╗██╗      █████╗ ██╗   ██╗██████╗ ███████╗${NC}`);
  console.log(`${BLUE}${BOLD}  ██╔════╝██║     ██╔══██╗██║   ██║██╔══██╗██╔════╝${NC}`);
  console.log(`${BLUE}${BOLD}  ██║     ██║     ███████║██║   ██║██║  ██║█████╗  ${NC}`);
  console.log(`${BLUE}${BOLD}  ██║     ██║     ██╔══██║██║   ██║██║  ██║██╔══╝  ${NC}`);
  console.log(`${BLUE}${BOLD}  ╚██████╗███████╗██║  ██║╚██████╔╝██████╔╝███████╗${NC}`);
  console.log(`${BLUE}${BOLD}   ╚═════╝╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚═════╝ ╚══════╝${NC}`);
  console.log(`${YELLOW}${BOLD}              MCP Toolkit — Setup${NC}`);
  console.log('');
}

function statusLabel(status, healthResults) {
  if (!status) return `${RED}❌${NC} (not configured)`;
  let label = `${GREEN}✅${NC} ${YELLOW}(${status})${NC}`;
  if (healthResults) {
    const expired = healthResults.filter(r => r.status === 'expired');
    if (expired.length > 0) label += ` ${RED}⚠ token expired${NC}`;
  }
  return label;
}

async function mainMenu() {
  const integrations = await loadIntegrations();

  let healthResults = null;
  const hasConfigured = integrations.some(i => getStatus(i));
  if (hasConfigured) {
    showBanner();
    console.log(`  ${GRAY}Checking tokens...${NC}`);
    healthResults = await checkAllTokens();
  }

  while (true) {
    showBanner();
    if (healthResults) printHealthResults(healthResults);

    const choices = integrations.map(i => {
      const matchingResults = healthResults
        ? healthResults.filter(r => i.configPrefix ? r.key.startsWith(i.configPrefix) : r.key === i.configKey)
        : null;
      return {
        name: `${i.name.padEnd(14)} ${statusLabel(getStatus(i), matchingResults)}`,
        value: i.key,
      };
    });

    choices.push(new inquirer.Separator());
    choices.push({ name: '🔧 Select multiple to set up', value: '__multi__' });
    choices.push({ name: '🔍 Check all tokens', value: '__check__' });
    choices.push({ name: '🗑️  Delete an integration', value: '__delete__' });
    choices.push({ name: '🧹 Purge config backups', value: '__purge__' });
    choices.push({ name: '🚪 Quit', value: '__quit__' });

    const { action } = await inquirer.prompt([{
      type: 'list',
      name: 'action',
      message: 'What would you like to do?',
      choices,
      loop: false,
      pageSize: 15,
    }]);

    if (action === '__quit__') {
      console.log(`\n  ${GREEN}${BOLD}All done!${NC}\n`);
      process.exit(0);
    }

    if (action === '__check__') {
      console.log(`\n  ${GRAY}Checking tokens...${NC}`);
      healthResults = await checkAllTokens();
      printHealthResults(healthResults);
      await waitForEnter();
      continue;
    }

    if (action === '__purge__') {
      await purgeBackupsFlow();
      await waitForEnter();
      continue;
    }

    if (action === '__multi__') {
      await selectMultiple(integrations);
      healthResults = await checkAllTokens();
      continue;
    }

    if (action === '__delete__') {
      const deleted = await deleteIntegration(integrations);
      if (deleted) {
        await restartClaude();
        await waitForEnter();
        healthResults = await checkAllTokens();
      }
      continue;
    }

    const integration = integrations.find(i => i.key === action);
    if (integration) {
      console.log('');
      const ok = await runSetup(integration);
      if (ok) await restartClaude();
      await waitForEnter();
      healthResults = await checkAllTokens();
    }
  }
}

// Backups mirror the config, so any token stored there in plaintext outlives
// its rotation. This lets the user clear that history on demand.
async function purgeBackupsFlow() {
  const { confirmed } = await inquirer.prompt([{
    type: 'confirm',
    name: 'confirmed',
    message: 'Delete all saved config backups? Tokens stored in them will be gone too.',
    default: false,
  }]);
  if (!confirmed) return;

  const removed = purgeBackups(configPath);
  console.log(removed > 0
    ? `${GREEN}  [ok] Deleted ${removed} backup(s)${NC}`
    : `${GRAY}  No backups to delete${NC}`);
}

// Runs an integration's setup() and converts unhandled errors into a friendly
// message so a failed `npm install -g` (e.g. offline) doesn't crash the toolkit.
async function runSetup(integration) {
  try {
    await integration.setup();
    return true;
  } catch (err) {
    if (err instanceof ExitPromptError) throw err;
    console.log(`\n  ${RED}Setup failed:${NC} ${err.message}`);
    console.log(`  ${YELLOW}You can try again from the menu. No changes were saved.${NC}`);
    return false;
  }
}

async function selectMultiple(integrations) {
  const choices = integrations.map(i => {
    const status = getStatus(i);
    return {
      name: `${i.name.padEnd(14)} ${status ? `${YELLOW}(${status})${NC}` : '(not configured)'}`,
      value: i.key,
      checked: !status,
    };
  });

  const { selected } = await inquirer.prompt([{
    type: 'checkbox',
    name: 'selected',
    message: 'Select integrations to set up (Space to toggle, Enter to confirm):',
    choices,
  }]);

  if (selected.length === 0) {
    console.log(`\n  ${YELLOW}Nothing selected${NC}`);
    return;
  }

  let anySucceeded = false;
  for (const key of selected) {
    const integration = integrations.find(i => i.key === key);
    console.log(`\n${BOLD}  ━━━ Setting up ${integration.name} ━━━${NC}\n`);
    if (await runSetup(integration)) anySucceeded = true;
  }

  if (anySucceeded) await restartClaude();
  await waitForEnter();
}

async function deleteIntegration(integrations) {
  const choices = integrations.map(i => {
    const status = getStatus(i);
    return {
      name: `${i.name.padEnd(14)} ${status ? `(${status})` : '(not configured)'}`,
      value: i.key,
      disabled: !status ? 'not configured' : false,
    };
  });
  choices.push({ name: '← Back', value: '__back__' });

  const { target } = await inquirer.prompt([{
    type: 'list',
    name: 'target',
    message: 'Which integration to delete?',
    choices,
  }]);

  if (target === '__back__') return false;

  const meta = integrations.find(i => i.key === target);
  const keysToDelete = deleteByMeta(meta);

  if (keysToDelete.length === 0) return false;

  console.log(`${GREEN}  [ok] Deleted: ${keysToDelete.join(', ')}${NC}`);
  return true;
}

mainMenu().catch(err => {
  if (err instanceof ExitPromptError) {
    console.log(`\n  ${GREEN}${BOLD}Bye!${NC}\n`);
    process.exit(0);
  }
  console.error(err);
  process.exit(1);
});
