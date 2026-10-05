import { exec, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import inquirer from 'inquirer';
import { setServer } from '../config.js';
import { mcpEntry } from '../npm.js';
import { GREEN, RED, YELLOW, NC } from '../colors.js';
import { promptIfConfigured } from '../single-instance.js';
import { safeFetch } from '../network.js';
import { offerSecretStorage } from '../op-storage.js';

const execAsync = promisify(exec);

function runInteractive(cmd, args) {
  return new Promise((resolve, reject) => {
    const proc = spawn(cmd, args, { stdio: 'inherit', shell: process.platform === 'win32' });
    proc.on('exit', code => code === 0 ? resolve() : reject(new Error(`${cmd} exited with code ${code}`)));
    proc.on('error', reject);
  });
}

async function installGhCli() {
  if (process.platform !== 'win32') {
    try {
      await execAsync('which brew');
    } catch {
      throw new Error('Homebrew not found. Install gh manually: https://cli.github.com/');
    }
    await execAsync('brew install gh');
    return;
  }

  try {
    await execAsync('winget install GitHub.cli --silent --accept-package-agreements --accept-source-agreements');
  } catch {
    console.log('  winget unavailable. Downloading latest GitHub CLI MSI from github.com...');
    const releaseResp = await safeFetch('https://api.github.com/repos/cli/cli/releases/latest', {
      headers: { 'User-Agent': 'claude-toolkit' },
    });
    if (!releaseResp) throw new Error('Failed to fetch GitHub CLI release info — check your internet connection');
    const release = await releaseResp.json();
    const asset = release.assets?.find(a => /^gh_.+_windows_amd64\.msi$/.test(a.name));
    if (!asset) throw new Error('No Windows MSI asset in latest gh release');

    const msi = path.join(os.tmpdir(), asset.name);
    const res = await safeFetch(asset.browser_download_url);
    if (!res) throw new Error('Failed to download GitHub CLI MSI — check your internet connection');
    fs.writeFileSync(msi, Buffer.from(await res.arrayBuffer()));

    console.log('  Installing (you may see a UAC prompt)...');
    try {
      await execAsync(`msiexec /i "${msi}" /passive /norestart`);
    } finally {
      fs.unlinkSync(msi);
    }
  }

  // Make gh.exe visible in PATH for this process (installer updates PATH only for new shells).
  const ghDirs = [
    path.join(process.env.ProgramFiles || '', 'GitHub CLI'),
    path.join(process.env.LOCALAPPDATA || '', 'Programs', 'GitHub CLI'),
  ];
  for (const dir of ghDirs) {
    if (fs.existsSync(path.join(dir, 'gh.exe'))) {
      process.env.PATH = dir + path.delimiter + process.env.PATH;
      break;
    }
  }
}

async function ghInstalled() {
  try {
    await execAsync('gh --version');
    return true;
  } catch {
    return false;
  }
}

async function ghAuthed() {
  try {
    await execAsync('gh auth status');
    return true;
  } catch {
    return false;
  }
}

// The new GitHub MCP server exposes a `projects` toolset (Projects v2), but the
// underlying GraphQL API needs a token carrying `read:project` (or `project`).
// `gh auth login`'s default scopes don't include it, so we parse the granted
// scopes off `gh auth status` and refresh to add it when missing.
async function ghTokenScopes() {
  try {
    const { stdout, stderr } = await execAsync('gh auth status');
    const m = (stdout + stderr).match(/Token scopes:\s*(.+)/);
    if (!m) return [];
    return [...m[1].matchAll(/'([^']+)'/g)].map(x => x[1]);
  } catch {
    return [];
  }
}

export const meta = {
  key: 'github',
  name: 'GitHub',
  configKey: 'github',
};

export async function setup() {
  if (!await promptIfConfigured('github', 'Refresh token')) return;

  // Check gh CLI
  if (!await ghInstalled()) {
    console.log(`${YELLOW}  GitHub CLI (gh) is not installed.${NC}\n`);

    const { install } = await inquirer.prompt([{
      type: 'confirm',
      name: 'install',
      message: 'Install GitHub CLI now?',
      default: true,
    }]);

    if (install) {
      console.log('\n  Installing GitHub CLI...');
      try {
        await installGhCli();
        console.log(`${GREEN}  [ok] GitHub CLI installed${NC}\n`);
      } catch (err) {
        console.log(`${RED}  Failed to install: ${err.message}${NC}`);
        console.log(`${RED}  Install manually: brew install gh (macOS) or https://cli.github.com/ (Windows)${NC}`);
        return;
      }
    } else {
      return;
    }
  }

  // Check auth
  if (!await ghAuthed()) {
    console.log('\n  GitHub CLI is not authorized. Starting login...\n');
    await runInteractive('gh', ['auth', 'login']).catch(() => {});

    if (!await ghAuthed()) {
      console.log(`${RED}  GitHub auth failed. Run "gh auth login" manually.${NC}`);
      return;
    }
  }

  console.log('\n  Getting token from GitHub CLI...');

  // Get token
  const { stdout: token } = await execAsync('gh auth token');
  const cleanToken = token.trim();

  if (!cleanToken) {
    console.log(`${RED}  Failed to get token from GitHub CLI${NC}`);
    return;
  }

  // Validate; refresh if the token is rejected.
  let tokenToUse = cleanToken;
  let refreshed = false;
  try {
    const { stdout: userJson } = await execAsync('gh api user');
    const user = JSON.parse(userJson);
    console.log(`${GREEN}  [ok] Validated (user: ${user.login})${NC}\n`);
  } catch {
    console.log(`${YELLOW}  Token may be expired. Refreshing...${NC}`);
    await execAsync('gh auth refresh');
    const { stdout: newToken } = await execAsync('gh auth token');
    tokenToUse = newToken.trim();
    refreshed = true;
  }

  // Ensure the token can read AND write Projects v2. The server filters tools by
  // OAuth scope, so without `project` (read+write) the projects_write tool stays
  // hidden — and this integration's whole point is creating project items. We
  // request the full `project` scope; `read:project` alone would be read-only.
  const scopes = await ghTokenScopes();
  if (!scopes.includes('project')) {
    console.log(`${YELLOW}  Token lacks Projects scope. Adding 'project'...${NC}`);
    try {
      await runInteractive('gh', ['auth', 'refresh', '-h', 'github.com', '-s', 'project']);
      const { stdout: scopedToken } = await execAsync('gh auth token');
      tokenToUse = scopedToken.trim();
      refreshed = true;
    } catch {
      console.log(`${YELLOW}  Could not add 'project' scope automatically.${NC}`);
      console.log(`${YELLOW}  Run "gh auth refresh -s project" if Projects tools are missing.${NC}`);
    }
  }

  // Migrated from the deprecated @modelcontextprotocol/server-github to GitHub's
  // official server (github/github-mcp-server). We bridge to its hosted endpoint
  // via mcp-remote, same as Linear. `projects` is NOT in the server's default
  // toolsets, so without the X-MCP-Toolsets header the Projects tools never show
  // up — we enable it explicitly alongside the everyday repo/issue/PR toolsets.
  const toolsets = 'context,repos,issues,pull_requests,users,projects';
  // Since October 2026 the hosted server answers issue_write and the pull
  // request write tools with a form to confirm instead of writing. Claude
  // Desktop does not draw that form (github/github-mcp-server#2823), so nothing
  // is ever written. This flag makes the tools write directly again.
  const features = 'mcp_apps_disable_form_deferral';
  const stored = await offerSecretStorage('github', tokenToUse, { envVar: 'GITHUB_MCP_TOKEN', ghSource: true });
  setServer('github', stored.apply({
    ...(await mcpEntry(
      'mcp-remote',
      'https://api.githubcopilot.com/mcp/',
      '--header', `Authorization: Bearer ${stored.value}`,
      '--header', `X-MCP-Toolsets: ${toolsets}`,
      '--header', `X-MCP-Features: ${features}`,
    )),
    env: {},
  }));
  console.log(`${GREEN}  [ok] GitHub configured${refreshed ? ' (token refreshed)' : ''}${NC}`);
}
