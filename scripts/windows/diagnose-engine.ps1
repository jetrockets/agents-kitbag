# Claude Toolkit — Diagnostic
# Collects everything needed to diagnose "MCP servers don't appear" issues
# on Windows (especially ES/PT locales). Writes a single text file to the
# Desktop with tokens masked, then opens Explorer on the file.
#
# Run by double-clicking diagnose.bat — that wraps this with chcp 65001
# and ExecutionPolicy Bypass.

$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# --- Output destination ---------------------------------------------------
$desktop = [Environment]::GetFolderPath('Desktop')
$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$outFile = Join-Path $desktop "diagnose-claude-$timestamp.txt"
$lines = New-Object System.Collections.ArrayList

function Add-Line { param([string]$Text = '') [void]$lines.Add($Text) }
function Add-Header {
    param([string]$Title)
    Add-Line ''
    Add-Line ('=' * 72)
    Add-Line "== $Title"
    Add-Line ('=' * 72)
}
function Add-Sub { param([string]$Title) Add-Line ''; Add-Line "--- $Title ---" }
function Try-Run {
    param([scriptblock]$Block, [string]$Label)
    try { & $Block }
    catch { Add-Line "[$Label] ERROR: $($_.Exception.Message)" }
}

# --- Token masking --------------------------------------------------------
# Replaces known secret patterns in any text. Keep first 4 chars of the
# token body so we can still see prefix/integration, mask the middle.
function Mask-Tokens {
    param([string]$Text)
    if (-not $Text) { return $Text }
    $patterns = @(
        # GitHub: gho_, ghp_, ghs_, ghu_, ghr_ + 30+ alphanumeric
        @{ Re = '(gh[oprsu]_)([A-Za-z0-9]{4})([A-Za-z0-9]{20,})'; Repl = '$1$2***MASKED***' },
        # Notion personal access token
        @{ Re = '(ntn_)([A-Za-z0-9]{4})([A-Za-z0-9]{30,})'; Repl = '$1$2***MASKED***' },
        @{ Re = '(secret_)([A-Za-z0-9]{4})([A-Za-z0-9]{30,})'; Repl = '$1$2***MASKED***' },
        # Figma
        @{ Re = '(figd_)([A-Za-z0-9_-]{4})([A-Za-z0-9_-]{30,})'; Repl = '$1$2***MASKED***' },
        # Linear
        @{ Re = '(lin_api_)([A-Za-z0-9]{4})([A-Za-z0-9]{30,})'; Repl = '$1$2***MASKED***' },
        # Atlassian / Jira
        @{ Re = '(ATATT)([A-Za-z0-9_-]{4})([A-Za-z0-9_-]{40,})'; Repl = '$1$2***MASKED***' },
        # Generic JSON token-like fields ("TOKEN":"xxxxxxxxxx...")
        @{ Re = '("(?:[A-Z_]*TOKEN|[A-Z_]*KEY|[A-Z_]*SECRET|PASSWORD)"\s*:\s*")([^"]{4})([^"]{12,})(")'; Repl = '$1$2***MASKED***$4' },
        # Bearer auth headers
        @{ Re = '(Bearer\s+)([A-Za-z0-9_\-]{4})([A-Za-z0-9_\-]{20,})'; Repl = '$1$2***MASKED***' }
    )
    $result = $Text
    foreach ($p in $patterns) {
        $result = [regex]::Replace($result, $p.Re, $p.Repl)
    }
    return $result
}

# --- Long-line wrap ------------------------------------------------------
# Wraps lines longer than $Width with a hard cut (no word splitting, since
# most overlong lines are JSON without spaces). Preserves existing newlines.
function Wrap-LongLine {
    param([string]$Text, [int]$Width = 160)
    if (-not $Text) { return $Text }
    $sb = New-Object System.Text.StringBuilder
    $first = $true
    foreach ($line in ($Text -split "`r?`n")) {
        if (-not $first) { [void]$sb.AppendLine() }
        $first = $false
        if ($line.Length -le $Width) {
            [void]$sb.Append($line)
        } else {
            $i = 0
            while ($i -lt $line.Length) {
                $chunk = $line.Substring($i, [Math]::Min($Width, $line.Length - $i))
                if ($i -gt 0) { [void]$sb.AppendLine() }
                [void]$sb.Append($chunk)
                $i += $Width
            }
        }
    }
    return $sb.ToString()
}

# --- 1. System ------------------------------------------------------------
Add-Header 'System'
Try-Run {
    Add-Line "Generated: $(Get-Date -Format o)"
    Add-Line "COMPUTERNAME: $env:COMPUTERNAME"
    $os = Get-CimInstance Win32_OperatingSystem
    Add-Line "OS: $($os.Caption) $($os.Version) (build $($os.BuildNumber))"
    Add-Line "OS locale: $($os.OSLanguage) / MUI: $($os.MUILanguages -join ', ')"
    Add-Line "PSVersion: $($PSVersionTable.PSVersion)"
    Add-Line "Culture: $((Get-Culture).Name)  UICulture: $((Get-UICulture).Name)"
    Add-Line "Preferred languages: $((Get-WinUserLanguageList | ForEach-Object { $_.LanguageTag }) -join ', ')"
    $cp = (chcp 2>$null) -replace '[^\d]', ''
    Add-Line "Console codepage (chcp): $cp"
    Add-Line "Console OutputEncoding: $([Console]::OutputEncoding.WebName) (CP $([Console]::OutputEncoding.CodePage))"
} 'system'

# --- 1b. Windows version details -----------------------------------------
Add-Header 'Windows version details'
Try-Run {
    $rv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue
    if ($rv) {
        Add-Line "EditionID:        $($rv.EditionID)"
        Add-Line "ProductName:      $($rv.ProductName)"
        Add-Line "DisplayVersion:   $($rv.DisplayVersion)"
        Add-Line "ReleaseId:        $($rv.ReleaseId)"
        Add-Line "CurrentBuild:     $($rv.CurrentBuild)"
        Add-Line "UBR:              $($rv.UBR)"
        Add-Line "BuildLabEx:       $($rv.BuildLabEx)"
        Add-Line "InstallationType: $($rv.InstallationType)"
    } else {
        Add-Line '(could not read CurrentVersion registry key)'
    }
} 'winver'

# --- 2. User & paths ------------------------------------------------------
Add-Header 'User & paths'
Try-Run {
    Add-Line "USERNAME: $env:USERNAME"
    $usernameHasNonAscii = $env:USERNAME -match '[^\x20-\x7E]'
    Add-Line "USERNAME has non-ASCII: $usernameHasNonAscii"
    if ($usernameHasNonAscii) {
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($env:USERNAME)
        Add-Line "USERNAME bytes (UTF-8 hex): $(($bytes | ForEach-Object { '{0:X2}' -f $_ }) -join ' ')"
    }
    Add-Line "USERPROFILE: $env:USERPROFILE"
    Add-Line "APPDATA:     $env:APPDATA"
    Add-Line "LOCALAPPDATA: $env:LOCALAPPDATA"
    Add-Line "TEMP: $env:TEMP"
    Add-Line "Desktop folder (GetFolderPath): $desktop"
    $pathHasNonAscii = $env:APPDATA -match '[^\x20-\x7E]'
    Add-Line "APPDATA contains non-ASCII: $pathHasNonAscii"
} 'user-paths'

# --- 3. Claude Desktop installations -------------------------------------
Add-Header 'Claude Desktop installations'
$script:msixFound = $false
$script:squirrelFound = $false
Try-Run {
    Add-Sub 'MSIX (Microsoft Store / sideloaded)'
    $msix = Get-AppxPackage -Name 'Claude' -ErrorAction SilentlyContinue
    if ($msix) {
        $script:msixFound = $true
        Add-Line "Name: $($msix.Name)"
        Add-Line "Version: $($msix.Version)"
        Add-Line "PackageFamilyName: $($msix.PackageFamilyName)"
        Add-Line "InstallLocation: $($msix.InstallLocation)"
        Add-Line "Architecture: $($msix.Architecture)"
        # Parse AppxManifest.xml — the technical answer to "is %APPDATA%
        # virtualized for this app". An MSIX without runFullTrust has its
        # %APPDATA% redirected to Packages\<id>\LocalCache\Roaming\ from
        # the app's own view.
        $manifestPath = Join-Path $msix.InstallLocation 'AppxManifest.xml'
        if (Test-Path $manifestPath) {
            try {
                $manifestRaw = Get-Content $manifestPath -Raw -ErrorAction Stop
                $hasRunFullTrust = $manifestRaw -match '(?i)runFullTrust'
                $hasUnvirtualized = $manifestRaw -match '(?i)unvirtualizedResources'
                Add-Line "Manifest path: $manifestPath"
                Add-Line "Has runFullTrust capability:   $hasRunFullTrust"
                Add-Line "Has unvirtualizedResources:    $hasUnvirtualized"
                if ($manifestRaw -match '<Identity\s+([^/]+)\s*/>') {
                    Add-Line (Wrap-LongLine "Manifest Identity: $($Matches[1])" 180)
                }
                [xml]$manifest = $manifestRaw
                $capNodes = @()
                foreach ($caps in $manifest.GetElementsByTagName('Capabilities')) {
                    foreach ($c in $caps.ChildNodes) {
                        if ($c.NodeType -eq 'Element') {
                            $n = if ($c.GetAttribute('Name')) { $c.GetAttribute('Name') } else { $c.LocalName }
                            $capNodes += "$($c.LocalName)=$n"
                        }
                    }
                }
                Add-Line "Capabilities: $($capNodes -join '; ')"
            } catch {
                Add-Line "(could not parse manifest: $($_.Exception.Message))"
            }
        } else {
            Add-Line "(AppxManifest.xml not found at $manifestPath)"
        }
    } else {
        Add-Line '(no MSIX install detected)'
    }
} 'msix'
Try-Run {
    Add-Sub 'Squirrel / .exe installer'
    $regPaths = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*'
    )
    $entries = Get-ItemProperty $regPaths -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -like '*Claude*' -and $_.DisplayName -notlike '*MCP*' }
    if ($entries) {
        foreach ($e in $entries) {
            Add-Line "DisplayName: $($e.DisplayName)"
            Add-Line "  Version: $($e.DisplayVersion)"
            Add-Line "  Publisher: $($e.Publisher)"
            Add-Line "  InstallLocation: $($e.InstallLocation)"
            Add-Line "  UninstallString: $($e.UninstallString)"
        }
        $script:squirrelFound = $true
    } else {
        Add-Line '(no Squirrel/installer-based Claude detected)'
    }
} 'squirrel'
Try-Run {
    Add-Sub 'Filesystem search for Claude.exe'
    $candidates = @(
        "$env:LOCALAPPDATA\AnthropicClaude",
        "$env:LOCALAPPDATA\Programs\AnthropicClaude",
        "$env:LOCALAPPDATA\Programs\Claude",
        "$env:LOCALAPPDATA\Claude",
        "$env:ProgramFiles\Claude",
        "${env:ProgramFiles(x86)}\Claude"
    )
    foreach ($c in $candidates) {
        if ($c -and (Test-Path $c)) {
            Add-Line "FOUND: $c"
            Get-ChildItem $c -Recurse -Filter 'Claude.exe' -ErrorAction SilentlyContinue |
                Select-Object -First 5 |
                ForEach-Object { Add-Line "  $($_.FullName) — $($_.VersionInfo.ProductVersion)" }
        }
    }
} 'fs-claude'
Add-Line ''
Add-Line "MSIX present: $script:msixFound"
Add-Line "Squirrel present: $script:squirrelFound"
Add-Line "MULTIPLE INSTALLATIONS: $($script:msixFound -and $script:squirrelFound)"

# --- 3b. 1Password CLI ----------------------------------------------------
# Servers configured to keep their token in 1Password launch as
# `op run -- <server>`. If op.exe is missing or 1Password is locked, those
# servers fail to start and Claude reports nothing more than a timeout.
Add-Header '1Password CLI (op)'
Try-Run {
    $opCandidates = @(
        "$env:LOCALAPPDATA\Microsoft\WinGet\Links\op.exe",
        "$env:ProgramFiles\1Password CLI\op.exe",
        "${env:ProgramFiles(x86)}\1Password CLI\op.exe",
        "$env:USERPROFILE\scoop\shims\op.exe",
        "$env:ProgramData\chocolatey\bin\op.exe"
    )
    $opPath = $opCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $opPath) {
        Add-Line 'op.exe: NOT FOUND (only affects servers configured to use 1Password)'
        return
    }
    Add-Line "op.exe: $opPath"
    Add-Line "version: $(& $opPath --version 2>&1)"

    # Not `op whoami`: with the desktop app integration it reports "not signed
    # in" while reads work fine. Listing vaults proves actual access.
    & $opPath vault list --format=json *> $null
    if ($LASTEXITCODE -eq 0) {
        Add-Line 'can reach vaults: yes'
    } else {
        Add-Line 'can reach vaults: NO — enable 1Password > Settings > Developer > Integrate with 1Password CLI, and unlock 1Password'
    }
} 'onepassword-cli'

# --- 4. Config file locations --------------------------------------------
Add-Header 'claude_desktop_config.json — all candidate locations'
$configCandidates = @(
    "$env:APPDATA\Claude\claude_desktop_config.json",
    "$env:LOCALAPPDATA\Claude\claude_desktop_config.json",
    "$env:LOCALAPPDATA\AnthropicClaude\claude_desktop_config.json"
)
# Add MSIX-redirected locations
Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like 'Claude*' -or $_.Name -like '*AnthropicClaude*' } |
    ForEach-Object {
        $configCandidates += "$($_.FullName)\LocalCache\Roaming\Claude\claude_desktop_config.json"
        $configCandidates += "$($_.FullName)\LocalState\Claude\claude_desktop_config.json"
        $configCandidates += "$($_.FullName)\RoamingState\Claude\claude_desktop_config.json"
    }
$script:foundConfigs = @()
foreach ($p in $configCandidates) {
    if (Test-Path $p) {
        Try-Run {
            $f = Get-Item $p -Force
            Add-Sub "FOUND: $p"
            Add-Line "Size: $($f.Length)  Modified: $($f.LastWriteTime)"
            Add-Line "Attributes: $($f.Attributes)"
            $isReparse = ($f.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
            Add-Line "File is reparse point: $isReparse"
            $dirItem = Get-Item (Split-Path $p -Parent) -Force -ErrorAction SilentlyContinue
            if ($dirItem) {
                $dirReparse = ($dirItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
                Add-Line "Containing dir reparse: $dirReparse"
                if ($dirReparse -and $dirItem.Target) {
                    Add-Line "Reparse target: $($dirItem.Target -join '; ')"
                }
            }
            try {
                $hash = (Get-FileHash -LiteralPath $p -Algorithm SHA256 -ErrorAction Stop).Hash.Substring(0, 16)
                Add-Line "SHA256 (first 16 hex): $hash"
            } catch {
                Add-Line "SHA256: (could not compute: $($_.Exception.Message))"
            }
            $content = Get-Content $p -Raw -ErrorAction Stop
            $masked = Mask-Tokens $content
            Add-Line 'Content (tokens masked):'
            Add-Line $masked
            try {
                $parsed = $content | ConvertFrom-Json
                $servers = ($parsed.mcpServers.PSObject.Properties.Name) -join ', '
                Add-Line "Parsed mcpServers: $servers"
            } catch {
                Add-Line "JSON PARSE ERROR: $($_.Exception.Message)"
            }
            $script:foundConfigs += $p
        } "config-$p"
    } else {
        Add-Line "(not present) $p"
    }
}

# Recursive search for any claude_desktop_config* anywhere under Packages
Try-Run {
    Add-Sub 'Recursive sweep under %LOCALAPPDATA%\Packages\'
    Get-ChildItem "$env:LOCALAPPDATA\Packages" -Recurse -Filter 'claude_desktop_config*' -ErrorAction SilentlyContinue |
        ForEach-Object { Add-Line "  $($_.FullName) ($($_.Length) bytes, $($_.LastWriteTime))" }
} 'sweep'

Add-Line ''
Add-Line "TOTAL CONFIG FILES FOUND: $($script:foundConfigs.Count)"

# Where does the toolkit write vs where does Claude actually read?
Add-Sub 'Path comparison: toolkit-side vs Claude-side'
$toolkitWritePath = "$env:APPDATA\Claude\claude_desktop_config.json"
Add-Line "Toolkit's write path: $toolkitWritePath  (exists: $([bool](Test-Path $toolkitWritePath)))"
$script:claudeReadPath = $null
$claudePkg = Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like 'Claude_*' } |
    Select-Object -First 1
if ($claudePkg) {
    $candidate = "$($claudePkg.FullName)\LocalCache\Roaming\Claude\claude_desktop_config.json"
    if (Test-Path $candidate) {
        $script:claudeReadPath = $candidate
        Add-Line "Claude MSIX-redirected path:    $candidate"
        Add-Line "  exists: True (Claude reads from here on Win11 MSIX)"
    } else {
        Add-Line "Claude MSIX-redirected path:    $candidate"
        Add-Line "  exists: False (not yet created — Claude not run, or no redirect on this OS)"
    }
}
if ($script:claudeReadPath) {
    try {
        $tw = Get-Content $toolkitWritePath -Raw -ErrorAction Stop | ConvertFrom-Json
        $cr = Get-Content $script:claudeReadPath -Raw -ErrorAction Stop | ConvertFrom-Json
        $twServers = if ($tw.mcpServers) { ($tw.mcpServers.PSObject.Properties.Name) -join ', ' } else { '' }
        $crServers = if ($cr.mcpServers) { ($cr.mcpServers.PSObject.Properties.Name) -join ', ' } else { '' }
        Add-Line "Toolkit-side mcpServers: $(if ($twServers) { $twServers } else { '(none)' })"
        Add-Line "Claude-side mcpServers:  $(if ($crServers) { $crServers } else { '(none)' })"
        if ($twServers -and -not $crServers) {
            Add-Line 'DIVERGENCE: toolkit wrote mcpServers, but Claude-side file has none.'
            Add-Line '             This IS the Windows 11 MSIX redirect bug.'
        } elseif ($twServers -ne $crServers) {
            Add-Line "DIVERGENCE: toolkit and Claude see DIFFERENT mcpServers."
        }
    } catch {
        Add-Line "(could not compare: $($_.Exception.Message))"
    }
}

# --- 5. Claude main logs --------------------------------------------------
Add-Header 'Claude main process logs'
$logDirs = @(
    "$env:APPDATA\Claude\logs",
    "$env:LOCALAPPDATA\Claude\logs",
    "$env:LOCALAPPDATA\AnthropicClaude\logs"
)
Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like 'Claude*' } |
    ForEach-Object { $logDirs += "$($_.FullName)\LocalCache\Roaming\Claude\logs" }

foreach ($d in $logDirs) {
    if (Test-Path $d) {
        Add-Sub "Log dir: $d"
        Get-ChildItem $d -Filter 'main*.log' -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1 |
            ForEach-Object {
                Add-Line "main log: $($_.FullName) ($($_.Length) bytes)"
                Try-Run {
                    $tail = Get-Content $_.FullName -Tail 60 -ErrorAction Stop
                    $masked = Mask-Tokens ($tail -join "`n")
                    Add-Line '--- last 60 lines ---'
                    Add-Line (Wrap-LongLine $masked 180)
                } "main-log"
            }
    }
}

# --- 6. MCP server logs ---------------------------------------------------
Add-Header 'MCP server logs (tail of each)'
foreach ($d in $logDirs) {
    if (Test-Path $d) {
        Get-ChildItem $d -Filter 'mcp-server-*.log' -ErrorAction SilentlyContinue |
            ForEach-Object {
                Add-Sub "$($_.Name) ($($_.Length) bytes, $($_.LastWriteTime))"
                Try-Run {
                    $tail = Get-Content $_.FullName -Tail 40 -ErrorAction Stop
                    $joined = ($tail -join "`n")
                    Add-Line (Wrap-LongLine (Mask-Tokens $joined) 180)
                } "mcp-log-$($_.Name)"
            }
    }
}

# --- 7. Process snapshot --------------------------------------------------
Add-Header 'Currently running claude.exe processes'
Try-Run {
    Get-Process -Name claude -ErrorAction SilentlyContinue | ForEach-Object {
        $p = $null
        try { $p = $_.MainModule.FileName } catch { $p = '(access denied)' }
        Add-Line ("  PID {0,6}  start {1}  path {2}" -f $_.Id, $_.StartTime, $p)
    }
} 'processes'

# --- 8. Node / npm --------------------------------------------------------
Add-Header 'Node.js / npm'
Try-Run {
    $nodeCmd = Get-Command node -ErrorAction SilentlyContinue
    if ($nodeCmd) {
        Add-Line "node path: $($nodeCmd.Source)"
        Add-Line "node version: $(& node --version 2>&1)"
    } else { Add-Line 'node: NOT FOUND in PATH' }
    $npmCmd = Get-Command npm -ErrorAction SilentlyContinue
    if ($npmCmd) {
        Add-Line "npm path: $($npmCmd.Source)"
        Add-Line "npm version: $(& npm --version 2>&1)"
        Add-Line "npm prefix: $(& npm prefix -g 2>&1)"
        Add-Line "npm root -g: $(& npm root -g 2>&1)"
        Add-Sub 'Global packages'
        & npm list -g --depth=0 2>&1 | ForEach-Object { Add-Line $_ }
    } else { Add-Line 'npm: NOT FOUND in PATH' }
} 'node-npm'

# --- 9. PATH --------------------------------------------------------------
Add-Header 'PATH'
$env:PATH -split ';' | ForEach-Object { if ($_) { Add-Line $_ } }

# --- 9b. Folder redirection / OneDrive KFM ------------------------------
Add-Header 'Folder redirection / OneDrive'
Try-Run {
    Add-Sub 'User Shell Folders (raw registry)'
    $usf = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders' -ErrorAction SilentlyContinue
    if ($usf) {
        foreach ($prop in 'Desktop', 'Personal', 'AppData', 'Local AppData', '{F42EE2D3-909F-4907-8871-4C22FC0BF756}') {
            $v = $usf.$prop
            if ($v) { Add-Line ("  {0,-50} {1}" -f $prop, $v) }
        }
    }
    Add-Sub 'OneDrive account state'
    $accounts = Get-ChildItem 'HKCU:\Software\Microsoft\OneDrive\Accounts' -ErrorAction SilentlyContinue
    if (-not $accounts) {
        Add-Line '(no OneDrive accounts configured)'
    } else {
        foreach ($a in $accounts) {
            $acc = Get-ItemProperty $a.PSPath -ErrorAction SilentlyContinue
            if ($acc) {
                Add-Line "Account: $($a.PSChildName)"
                Add-Line "  UserFolder: $($acc.UserFolder)"
                if ($acc.ServiceEndpointUri) { Add-Line "  ServiceEndpointUri: $($acc.ServiceEndpointUri)" }
                # Known Folder Move flags
                $names = $acc.PSObject.Properties.Name | Where-Object { $_ -like 'KFM*' }
                if ($names) { Add-Line "  KFM keys: $($names -join ', ')" }
            }
        }
    }
} 'redirection'

# --- 11. Bin reachability -------------------------------------------------
Add-Header 'MCP server bin reachability test'
foreach ($cfgPath in $script:foundConfigs) {
    Try-Run {
        $cfg = Get-Content $cfgPath -Raw | ConvertFrom-Json
        Add-Sub "From config: $cfgPath"
        foreach ($name in $cfg.mcpServers.PSObject.Properties.Name) {
            $srv = $cfg.mcpServers.$name
            $cmd = $srv.command
            $firstArg = if ($srv.args -and $srv.args.Count -gt 0) { $srv.args[0] } else { $null }
            Add-Line "Server '$name':"
            Add-Line "  command: $cmd  (exists: $([bool](Test-Path -LiteralPath $cmd 2>$null)))"
            if ($firstArg) {
                if ($firstArg -like '*.js' -or $firstArg -like '*\*' -or $firstArg -like '*/*') {
                    Add-Line "  arg[0]:  $firstArg  (exists: $([bool](Test-Path -LiteralPath $firstArg 2>$null)))"
                } else {
                    Add-Line "  arg[0]:  $firstArg"
                }
            }
        }
    } "bin-check-$cfgPath"
}

# --- 12. Heuristic verdict -----------------------------------------------
Add-Header 'Heuristic summary'
$verdicts = New-Object System.Collections.ArrayList
if ($script:msixFound -and $script:squirrelFound) { [void]$verdicts.Add('CRITICAL: BOTH MSIX and Squirrel Claude installed. They have separate config locations.') }
if ($script:foundConfigs.Count -eq 0) { [void]$verdicts.Add('CRITICAL: no claude_desktop_config.json found anywhere.') }
if ($script:foundConfigs.Count -gt 1) { [void]$verdicts.Add("WARNING: $($script:foundConfigs.Count) config files in different locations. Claude may read a different one than the toolkit wrote.") }
if ($env:USERNAME -match '[^\x20-\x7E]') { [void]$verdicts.Add('NOTE: USERNAME contains non-ASCII characters — bin paths in config will too.') }
# Win 11 + MSIX redirect bug detection
$twPath = "$env:APPDATA\Claude\claude_desktop_config.json"
$claudePkgDir = Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like 'Claude_*' } | Select-Object -First 1
if ($claudePkgDir) {
    $crPath = "$($claudePkgDir.FullName)\LocalCache\Roaming\Claude\claude_desktop_config.json"
    if ((Test-Path $twPath) -and (Test-Path $crPath)) {
        try {
            $tw = Get-Content $twPath -Raw | ConvertFrom-Json
            $cr = Get-Content $crPath -Raw | ConvertFrom-Json
            $hasTw = [bool]($tw.mcpServers -and $tw.mcpServers.PSObject.Properties.Name)
            $hasCr = [bool]($cr.mcpServers -and $cr.mcpServers.PSObject.Properties.Name)
            if ($hasTw -and -not $hasCr) {
                [void]$verdicts.Add('CRITICAL: toolkit wrote mcpServers to %APPDATA%\Claude\, but Claude reads MSIX-redirected path which has only preferences. This is the Windows 11 MSIX redirect bug.')
            }
        } catch {}
    }
}
if ($verdicts.Count -eq 0) { Add-Line '(no obvious red flags from this snapshot)' }
else { foreach ($v in $verdicts) { Add-Line $v } }

# --- Write file ----------------------------------------------------------
$utf8Bom = New-Object System.Text.UTF8Encoding $true
[System.IO.File]::WriteAllLines($outFile, $lines, $utf8Bom)

# --- Friendly final message ---------------------------------------------
$lang = (Get-Culture).TwoLetterISOLanguageName
switch ($lang) {
    'es' {
        Write-Host ''
        Write-Host '  ============================================================' -ForegroundColor Green
        Write-Host '  Diagnóstico completado.' -ForegroundColor Green
        Write-Host ''
        Write-Host "  Archivo guardado en: $outFile" -ForegroundColor Yellow
        Write-Host ''
        Write-Host '  Envíe este archivo al administrador / desarrollador.'
        Write-Host '  (Se abrirá el Explorador resaltando el archivo.)'
        Write-Host '  ============================================================' -ForegroundColor Green
    }
    'pt' {
        Write-Host ''
        Write-Host '  ============================================================' -ForegroundColor Green
        Write-Host '  Diagnóstico concluído.' -ForegroundColor Green
        Write-Host ''
        Write-Host "  Arquivo salvo em: $outFile" -ForegroundColor Yellow
        Write-Host ''
        Write-Host '  Envie este arquivo ao administrador / desenvolvedor.'
        Write-Host '  (O Explorador será aberto destacando o arquivo.)'
        Write-Host '  ============================================================' -ForegroundColor Green
    }
    'ru' {
        Write-Host ''
        Write-Host '  ============================================================' -ForegroundColor Green
        Write-Host '  Диагностика завершена.' -ForegroundColor Green
        Write-Host ''
        Write-Host "  Файл сохранён: $outFile" -ForegroundColor Yellow
        Write-Host ''
        Write-Host '  Перешлите этот файл администратору / разработчику.'
        Write-Host '  (Сейчас откроется Проводник с подсвеченным файлом.)'
        Write-Host '  ============================================================' -ForegroundColor Green
    }
    default {
        Write-Host ''
        Write-Host '  ============================================================' -ForegroundColor Green
        Write-Host '  Diagnostic complete.' -ForegroundColor Green
        Write-Host ''
        Write-Host "  File saved: $outFile" -ForegroundColor Yellow
        Write-Host ''
        Write-Host '  Please send this file to your administrator / developer.'
        Write-Host '  (Explorer will open highlighting the file.)'
        Write-Host '  ============================================================' -ForegroundColor Green
    }
}

# Open Explorer on the file
Start-Process explorer.exe "/select,`"$outFile`""
