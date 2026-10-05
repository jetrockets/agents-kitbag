# setup.ps1 — Claude MCP Toolkit setup logic for Windows.
# Called by "Start Windows.bat". Testable with Pester.

param(
    [string]$ProjectRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
)

function Test-ZipLaunch {
    -not (Test-Path (Join-Path $ProjectRoot 'package.json'))
}

function Test-NodeInstalled {
    $null -ne (Get-Command 'node' -ErrorAction SilentlyContinue)
}

function Test-WingetAvailable {
    $null -ne (Get-Command 'winget' -ErrorAction SilentlyContinue)
}

function Test-DependenciesInstalled {
    Test-Path (Join-Path $ProjectRoot 'node_modules\inquirer\package.json')
}

function Test-SourceExists {
    Test-Path (Join-Path $ProjectRoot 'src\index.js')
}

function Install-NodeViaWinget {
    winget install OpenJS.NodeJS.LTS --silent --accept-package-agreements --accept-source-agreements
}

function Install-NodeViaMsi {
    $installScript = Join-Path $ProjectRoot 'scripts\windows\install-node.ps1'
    & $installScript
    return $LASTEXITCODE
}

function Update-NodePath {
    $nodePath = Join-Path $env:ProgramFiles 'nodejs'
    if (Test-Path (Join-Path $nodePath 'node.exe')) {
        $env:PATH = "$nodePath;$env:PATH"
    }
}

function Install-Node {
    Write-Host ''
    Write-Host '  Node.js is not installed. Installing...'
    Write-Host ''

    if (Test-WingetAvailable) {
        Install-NodeViaWinget
    }
    else {
        Write-Host '  winget not available. Falling back to direct download from nodejs.org...'
        Write-Host ''
        $exitCode = Install-NodeViaMsi
        if ($exitCode -ne 0) {
            Write-Host ''
            Write-Host '  Failed to install Node.js automatically.'
            Write-Host '  Please install manually: https://nodejs.org/'
            return $false
        }
    }

    Update-NodePath

    if (-not (Test-NodeInstalled)) {
        Write-Host ''
        Write-Host '  Node.js was installed but is not visible in this session.'
        Write-Host '  Close this window, open a new terminal, and run "Start Windows.bat" again.'
        return $false
    }

    Write-Host ''
    Write-Host '  Node.js installed successfully.'
    Write-Host ''
    return $true
}

function Invoke-Setup {
    if (Test-ZipLaunch) {
        Write-Host ''
        Write-Host '  ERROR: package.json not found in current directory.'
        Write-Host ''
        Write-Host '  It looks like you ran "Start Windows.bat" directly from a ZIP file.'
        Write-Host '  Windows cannot run scripts from inside a ZIP archive.'
        Write-Host ''
        Write-Host '  To fix:'
        Write-Host '    1. Right-click the ZIP file > "Extract All..."'
        Write-Host '    2. Open the extracted folder'
        Write-Host '    3. Double-click "Start Windows.bat"'
        Write-Host ''
        return 1
    }

    if (-not (Test-NodeInstalled)) {
        $result = Install-Node
        if (-not $result) { return 1 }
    }

    if (-not (Test-DependenciesInstalled)) {
        Write-Host '  Installing dependencies for the first time, please wait...'
        & npm install --silent
    }

    if (-not (Test-SourceExists)) {
        Write-Host ''
        Write-Host '  ERROR: src\index.js not found. The download may be incomplete.'
        Write-Host '  Please re-download and extract the full ZIP archive.'
        Write-Host ''
        return 1
    }

    return 0
}

# setup.ps1 only installs Node + npm dependencies. The actual menu is launched
# by "Start Windows.bat" directly after this script returns — that way node.exe
# runs as a child of cmd.exe (Consolas font) instead of a grandchild of
# powershell.exe (which would make conhost switch the window font to
# PowerShell's per-exe setting, Lucida Console).
if ($MyInvocation.InvocationName -ne '.') {
    $exitCode = Invoke-Setup
    exit $exitCode
}
