# install-node.ps1 — downloads and installs latest Node.js LTS via MSI.
# Called by "Start Windows.bat" when winget is unavailable.
# Exits 0 on success, 1 on failure.

$ErrorActionPreference = 'Stop'

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    Write-Host "  Fetching Node.js release index..."
    $releases = Invoke-RestMethod -Uri 'https://nodejs.org/dist/index.json' -UseBasicParsing
    $lts = $releases | Where-Object { $_.lts } | Select-Object -First 1

    if (-not $lts) {
        Write-Host "  ERROR: could not determine latest LTS version." -ForegroundColor Red
        exit 1
    }

    $version = $lts.version
    $url = "https://nodejs.org/dist/$version/node-$version-x64.msi"
    $msi = Join-Path $env:TEMP "node-lts-x64.msi"

    Write-Host "  Downloading Node.js $version ($($lts.lts) LTS)..."
    Remove-Item $msi -Force -ErrorAction SilentlyContinue
    Invoke-WebRequest -Uri $url -OutFile $msi -UseBasicParsing

    Write-Host "  Installing Node.js (you may see a UAC prompt)..."
    $proc = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', "`"$msi`"", '/passive', '/norestart') -Wait -PassThru
    Remove-Item $msi -Force -ErrorAction SilentlyContinue

    if ($proc.ExitCode -ne 0) {
        Write-Host "  ERROR: msiexec exited with code $($proc.ExitCode)." -ForegroundColor Red
        exit 1
    }

    Write-Host "  Node.js installed."
    exit 0
}
catch {
    Write-Host "  ERROR: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
