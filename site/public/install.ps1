<#
.SYNOPSIS
  flowy-agent-store one-line installer (Windows x64).

.DESCRIPTION
  Installs the Agent Store runtime binary from npm
  (@flowy-agent-store/runtime-win32-x64) and puts flowy-agent-store.exe on
  your user PATH. No admin rights required.

  One-line (default, latest version):
    irm <script-url> | iex

  Pinned version (download first so -Version can be passed):
    iwr <script-url> -OutFile install-flowy-agent-store.ps1
    ./install-flowy-agent-store.ps1 -Version 0.1.0-beta.2

.NOTES
  Requires Node.js LTS (npm) on PATH. Windows on ARM runs the x64 binary
  through emulation.
#>
param(
  # npm version or dist-tag to install ("latest" by default).
  [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"

if (-not $env:LOCALAPPDATA) {
  throw "This installer targets Windows (LOCALAPPDATA not found)."
}
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
  throw "npm not found. Install Node.js LTS from https://nodejs.org first."
}

# The published Windows target is win32-x64; ARM64 devices run it via emulation.
if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
  Write-Warning "Windows on ARM detected: installing the x64 build (runs via emulation)."
}
$pkg = "@flowy-agent-store/runtime-win32-x64"

Write-Host "==> Installing $pkg ($Version) from npm"
if ($Version -eq "latest") {
  npm install -g $pkg --no-fund --no-audit
} else {
  npm install -g "$pkg@$Version" --no-fund --no-audit
}
if ($LASTEXITCODE -ne 0) {
  throw "npm install failed with exit code $LASTEXITCODE"
}

$prefix = (npm prefix -g).Trim()
$pkgDir = Join-Path $prefix "node_modules\$pkg\vendor"

# Newer packages vendor `flowy-agent-store.exe`; older ones (<= 0.1.0-beta.2)
# shipped `agent-store.exe`. Either way the installed command is unified as
# flowy-agent-store.
$src = Join-Path $pkgDir "flowy-agent-store.exe"
if (-not (Test-Path $src)) {
  $src = Join-Path $pkgDir "agent-store.exe"
}
if (-not (Test-Path $src)) {
  throw "Installed package is missing the runtime binary (looked in $pkgDir)"
}

# Copy the single-file binary to a stable location that survives npm updates.
$installDir = Join-Path $env:LOCALAPPDATA "Programs\flowy-agent-store"
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item $src (Join-Path $installDir "flowy-agent-store.exe") -Force
Write-Host "==> Binary installed to $installDir"

# Append to the *user* PATH (no admin needed); skip when already present.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $installDir) {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) {
    $installDir
  } else {
    $userPath.TrimEnd(";") + ";" + $installDir
  }
  [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
  $env:Path += ";$installDir"
  Write-Host "==> Added $installDir to your user PATH"
} else {
  Write-Host "==> $installDir is already on your user PATH"
}

Write-Host ""
Write-Host "Done. Open a NEW terminal window and run:"
Write-Host "  flowy-agent-store"
