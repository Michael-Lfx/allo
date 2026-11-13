[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root

$dxCommand = Get-Command dx -CommandType Application -ErrorAction SilentlyContinue
if ($null -eq $dxCommand) {
    Write-Error "dx not found. Install: cargo install dioxus-cli --version 0.7.10 --locked"
    exit 1
}

$dxPath = $dxCommand.Source
$versionOutput = (& $dxPath --version 2>&1 | Out-String).Trim()
$versionExitCode = $LASTEXITCODE
if ($versionExitCode -ne 0 -or $versionOutput -notmatch "\b0\.7\.10\b") {
    Write-Error "need dx 0.7.10 (gpui subsecond), got: $versionOutput"
    exit 1
}

$windowsLinker = Get-Command link.exe -CommandType Application -ErrorAction SilentlyContinue
if ($null -eq $windowsLinker) {
    Write-Error "MSVC link.exe not found. Run this from a Visual Studio Developer PowerShell or install the MSVC Build Tools."
    exit 1
}

$linkerPath = "link.exe"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $linkerPath
$env:CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER = $linkerPath

$msvcRoot = Split-Path (Split-Path (Split-Path (Split-Path $windowsLinker.Source -Parent) -Parent) -Parent) -Parent
$msvcLib = Join-Path $msvcRoot "lib\x64"
$programFilesX86 = [Environment]::GetEnvironmentVariable("ProgramFiles(x86)")
$windowsKitsRoot = Join-Path $programFilesX86 "Windows Kits\10\Lib"
$sdkLib = Get-ChildItem $windowsKitsRoot -Directory -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName "um\x64\advapi32.lib") } |
    Sort-Object Name -Descending |
    Select-Object -First 1
if ($null -eq $sdkLib) {
    Write-Error "Windows SDK advapi32.lib not found. Install the Windows 10/11 SDK through Visual Studio Build Tools."
    exit 1
}

$libPaths = @(
    $msvcLib
    (Join-Path $sdkLib.FullName "um\x64")
)
if (-not [string]::IsNullOrWhiteSpace($env:LIB)) {
    $libPaths += $env:LIB
}
$env:LIB = $libPaths -join ";"

$runningFlowy = Get-Process -Name flowy_app -ErrorAction SilentlyContinue
if ($null -ne $runningFlowy) {
    $pids = ($runningFlowy | Select-Object -ExpandProperty Id) -join ", "
    Write-Error "flowy-app.exe is already running (PID: $pids). Close the existing Flowy windows before starting dx serve."
    exit 1
}

$helpOutput = (& $dxPath serve --help 2>&1 | Out-String)
$helpExitCode = $LASTEXITCODE
if ($helpExitCode -ne 0) {
    Write-Error "failed to inspect dx serve options: $helpOutput"
    exit 1
}

$hotPatchFlag = $null
if ($helpOutput -match "--hotpatch(?:\s|,|$)") {
    $hotPatchFlag = "--hotpatch"
} elseif ($helpOutput -match "--hot-patch(?:\s|,|$)") {
    $hotPatchFlag = "--hot-patch"
}

if ($null -eq $hotPatchFlag) {
    Write-Error "dx serve does not expose --hotpatch or --hot-patch. Install dioxus-cli 0.7.10."
    exit 1
}

$dxArgs = @(
    "serve",
    $hotPatchFlag,
    "--package", "flowy-desktop",
    "--bin", "flowy-app",
    "--platform", "windows",
    "--renderer", "native",
    "--open", "false"
)

& $dxPath @dxArgs
exit $LASTEXITCODE
