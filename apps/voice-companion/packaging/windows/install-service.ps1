# Flowy Voice Companion Windows Service Installation Script (Wave 4 T4-02)
# Requires Administrator privilege

$ErrorActionPreference = "Stop"

$ServiceName = "FlowyVoiceCompanion"
$DisplayName = "Flowy Voice Companion Service"
$Description = "Flowy headless voice companion daemon"
$InstallDir = "$env:ProgramFiles\Flowy\voice-companion"
$DataDir = "$env:ProgramData\Flowy\voice-companion"
$BinaryPath = "$InstallDir\voice-companion.exe"

Write-Host "Installing $DisplayName..."

# 1. Ensure directories exist
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path "$DataDir\models" | Out-Null

# 2. Copy binary if present in current dir
if (Test-Path ".\target\release\voice-companion.exe") {
    Copy-Item -Force ".\target\release\voice-companion.exe" $BinaryPath
} elseif (Test-Path ".\voice-companion.exe") {
    Copy-Item -Force ".\voice-companion.exe" $BinaryPath
} elseif (Test-Path ".\target\release\ai-mate.exe") {
    Copy-Item -Force ".\target\release\ai-mate.exe" $BinaryPath
}

# 3. Register or Update Service
$existingService = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existingService) {
    Write-Host "Stopping existing service $ServiceName..."
    Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue
    & sc.exe config $ServiceName binPath= "`"$BinaryPath`" --config `"$DataDir\config.toml`"" start= auto
} else {
    Write-Host "Creating service $ServiceName..."
    & sc.exe create $ServiceName binPath= "`"$BinaryPath`" --config `"$DataDir\config.toml`"" start= auto DisplayName= "$DisplayName"
    & sc.exe description $ServiceName "$Description"
}

# 4. Set failure recovery to restart service
& sc.exe failure $ServiceName reset= 86400 actions= restart/5000/restart/5000/restart/5000

Write-Host "Flowy Voice Companion Windows Service configured successfully."
