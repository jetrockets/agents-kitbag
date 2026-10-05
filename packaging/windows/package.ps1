# Put the two programs and the update marker in the portable archive
# fastframe-update looks for:
#
#   claude-toolkit-v<version>-x86_64-pc-windows-msvc.zip
#     claude-toolkit-v<version>-x86_64-pc-windows-msvc\
#       claude-toolkit.exe
#       claude-toolkit-runner.exe
#       claude-toolkit-portable.txt
#
#   packaging\windows\package.ps1 -BinDir target\release -Version 0.21.0 -OutDir dist
#
# The marker is what lets a copy replace itself: without it beside the
# executable, fastframe-update treats the copy as someone else's to manage.
# A self-update replaces claude-toolkit.exe only. The runner's command line
# is a contract and does not change between versions, so the one that came
# with the first download keeps working.
param(
    [Parameter(Mandatory)] [string] $BinDir,
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $OutDir
)
$ErrorActionPreference = 'Stop'

$name = "claude-toolkit-v$Version-x86_64-pc-windows-msvc"
$staging = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
$folder = Join-Path $staging $name
New-Item -ItemType Directory -Force -Path $folder | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Copy-Item (Join-Path $BinDir 'claude-toolkit.exe') $folder
Copy-Item (Join-Path $BinDir 'claude-toolkit-runner.exe') $folder
Set-Content -Path (Join-Path $folder 'claude-toolkit-portable.txt') -Value 'claude-toolkit-portable-v1' -NoNewline

$zip = Join-Path $OutDir "$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $folder -DestinationPath $zip
Remove-Item -Recurse -Force $staging

Write-Output $zip
