import { exec } from 'node:child_process';
import { promisify } from 'node:util';
import { isWindows } from './platform.js';
import { BLUE, BOLD, CYAN, GREEN, NC } from './colors.js';

const execAsync = promisify(exec);

async function isClaudeRunning() {
  try {
    if (isWindows) {
      const { stdout } = await execAsync('tasklist /FI "IMAGENAME eq Claude.exe" /NH');
      return stdout.includes('Claude.exe');
    }
    await execAsync('pgrep -x Claude');
    return true;
  } catch {
    return false;
  }
}

export async function restartClaude() {
  // TODO(windows): implement a proper restart for MSIX-packaged Claude Desktop.
  // The previous taskkill /F + `start "" Claude` approach has several issues:
  //   - relaunch loses MSIX packaged-app context (env vars, working dir),
  //     occasionally leaving the app in a half-initialized state;
  //   - any in-flight chat session is torn down without warning;
  //   - on some machines `start "" "Claude"` resolves to a stale shortcut and
  //     the app doesn't come back at all.
  // A reliable path likely needs `explorer.exe shell:AppsFolder\<PackageFamilyName>!App`
  // with the family name discovered from `Get-AppxPackage`. Until that's built
  // and tested, fall through to manual instructions on Windows.
  if (isWindows) {
    console.log('');
    console.log(`${CYAN}  ──────────────────────────────────────────${NC}`);
    console.log(`${BOLD}  Next step:${NC}`);
    console.log(`${CYAN}    1.${NC} Fully quit Claude Desktop (right-click tray icon → Quit)`);
    console.log(`${CYAN}    2.${NC} Open Claude Desktop again`);
    console.log(`${CYAN}    3.${NC} Your MCP servers will be available in new chats`);
    console.log(`${CYAN}  ──────────────────────────────────────────${NC}`);
    return;
  }

  if (!await isClaudeRunning()) {
    console.log(`${BLUE}  Claude Desktop is not running. Launch it to use MCP tools.${NC}`);
    return;
  }

  console.log(`${BLUE}  Restarting Claude Desktop...${NC}`);
  await execAsync('pkill -x Claude').catch(() => {});

  for (let i = 0; i < 20; i++) {
    if (!await isClaudeRunning()) break;
    await new Promise(r => setTimeout(r, 500));
  }

  exec('open -a "Claude"');
  console.log(`${GREEN}  [ok] Claude Desktop restarted${NC}`);
}
