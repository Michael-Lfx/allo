[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root

# dx launches a copy with a hash in the executable name.
$runningFlowy = Get-Process -Name 'flowy-app', 'flowy-app-*', 'flowy_app' -ErrorAction SilentlyContinue
if ($null -ne $runningFlowy) {
    $pids = ($runningFlowy | Select-Object -ExpandProperty Id) -join ", "
    Write-Error "Flowy is already running (PID: $pids). Close the existing Flowy windows and stop any previous dx serve session before starting again."
    exit 1
}

# Dioxus hot-patching captures rustc invocations through its own workspace
# wrapper. A user-level RUSTC_WRAPPER=sccache (and cargo [env] force=true)
# hides that capture. Clear both here; the dx capture wrapper is installed
# just before serve.
Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
Remove-Item Env:RUSTC_WORKSPACE_WRAPPER -ErrorAction SilentlyContinue
Remove-Item Env:__CARGO_TEST_FORCE_ARGFILE -ErrorAction SilentlyContinue

$dxCommands = @(Get-Command dx.exe, dx -CommandType Application -ErrorAction SilentlyContinue | Where-Object { $_.Source -like '*.exe' })
$dxCommand = ($dxCommands | Where-Object { $_.Source -notmatch '\\mise\\shims\\' } | Select-Object -First 1)
if ($null -eq $dxCommand) {
    $dxCommand = $dxCommands | Select-Object -First 1
}
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

function Resolve-MsvcLinker {
    $candidates = Get-Command link.exe -CommandType Application -All -ErrorAction SilentlyContinue |
        Where-Object { $_.Source -match '\\VC\\Tools\\MSVC\\' }
    if ($candidates) {
        return ($candidates | Select-Object -First 1).Source
    }

    $programFilesX86 = [Environment]::GetEnvironmentVariable("ProgramFiles(x86)")
    if (-not [string]::IsNullOrWhiteSpace($programFilesX86)) {
        $vswhere = Join-Path $programFilesX86 "Microsoft Visual Studio\Installer\vswhere.exe"
        if (Test-Path -LiteralPath $vswhere) {
            $vsPath = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Out-String).Trim()
            if (-not [string]::IsNullOrWhiteSpace($vsPath)) {
                $found = Get-ChildItem -Path (Join-Path $vsPath "VC\Tools\MSVC\*\bin\Hostx64\x64\link.exe") -ErrorAction SilentlyContinue |
                    Sort-Object FullName -Descending |
                    Select-Object -First 1
                if ($null -ne $found) {
                    return $found.FullName
                }
            }
        }
    }
    return $null
}

$linkerPath = Resolve-MsvcLinker
if ([string]::IsNullOrWhiteSpace($linkerPath)) {
    Write-Error "MSVC link.exe not found. Install the Visual Studio MSVC Build Tools or run from a Visual Studio Developer PowerShell."
    exit 1
}

# Prepend the MSVC Hostx64\x64 directory so link.exe/lib.exe/cl.exe are ahead of
# any MSYS /usr/bin coreutils helpers (whose `link.exe` causes LNK1104 / exit 1104).
$msvcBinDir = Split-Path -Parent $linkerPath
$env:PATH = "$msvcBinDir;$env:PATH"

$msvcRoot = Split-Path (Split-Path (Split-Path (Split-Path $linkerPath -Parent) -Parent) -Parent) -Parent
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

$sdkUcrt = Join-Path $sdkLib.FullName "ucrt\x64"
$sdkUm = Join-Path $sdkLib.FullName "um\x64"
$requiredLibraries = @(
    (Join-Path $msvcLib "vcruntime.lib")
    (Join-Path $sdkUcrt "ucrt.lib")
    (Join-Path $sdkUm "advapi32.lib")
)
if ($requiredLibraries | Where-Object { -not (Test-Path $_) }) {
    Write-Error "MSVC/Windows SDK library set is incomplete. Expected vcruntime.lib, ucrt.lib, and advapi32.lib for x64."
    exit 1
}

$libPaths = @(
    $msvcLib
    $sdkUcrt
    $sdkUm
)
if (-not [string]::IsNullOrWhiteSpace($env:LIB)) {
    $libPaths += $env:LIB
}
$env:LIB = $libPaths -join ";"

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

$hostLine = & rustc -vV | Select-String -Pattern '^host:\s+'
if ($null -eq $hostLine) {
    Write-Error "failed to read the rustc host triple"
    exit 1
}
$triple = ($hostLine.ToString() -replace '^host:\s+', '').Trim()
$sysroot = (& rustc --print sysroot | Out-String).Trim()
$lldLinkPath = Join-Path $sysroot "lib\rustlib\$triple\bin\gcc-ld\lld-link.exe"
# Prefer rustup's bundled lld-link.exe (dx 0.7.10's native Windows hot-patch
# linker): it handles GNU ar fat archives (libdeps-*.a) without LNK4003, never
# emits MSVC /INCREMENTAL @ILT thunks that break subsecond's ASLR offset, and
# links fat/patch binaries significantly faster. Fall back to MSVC link.exe
# (with /INCREMENTAL:NO forced by dx-link-wrapper) if lld-link.exe is absent.
$realLinkerPath = if (Test-Path -LiteralPath $lldLinkPath) {
    $lldLinkPath
} else {
    $linkerPath
}

$wrapperManifest = Join-Path $root "scripts\dx-capture-wrapper\Cargo.toml"
$wrapperTarget = Join-Path $root "scripts\dx-capture-wrapper\target"
$wrapperExe = Join-Path $wrapperTarget "release\dx-capture-wrapper.exe"
$linkWrapperExe = Join-Path $wrapperTarget "release\dx-link-wrapper.exe"
$captureSources = @(
    $wrapperManifest
    (Join-Path $root "scripts\dx-capture-wrapper\src\main.rs")
)
$linkSources = @(
    $wrapperManifest
    (Join-Path $root "scripts\dx-capture-wrapper\src\link_wrapper.rs")
)
$newestCaptureSource = ($captureSources | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTimeUtc } | Sort-Object -Descending | Select-Object -First 1)
$newestLinkSource = ($linkSources | ForEach-Object { (Get-Item -LiteralPath $_).LastWriteTimeUtc } | Sort-Object -Descending | Select-Object -First 1)
$wrapperStale =
    (-not (Test-Path -LiteralPath $wrapperExe)) -or
    (-not (Test-Path -LiteralPath $linkWrapperExe)) -or
    ($newestCaptureSource -gt (Get-Item -LiteralPath $wrapperExe).LastWriteTimeUtc) -or
    ($newestLinkSource -gt (Get-Item -LiteralPath $linkWrapperExe).LastWriteTimeUtc)
if ($wrapperStale) {
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $linkerPath
    $env:CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER = $linkerPath
    # Override the parent repo's [build] build-dir without --config quoting
    # issues under Windows PowerShell 5.1, then restore it before dx serve.
    $prevBuildDir = $env:CARGO_BUILD_BUILD_DIR
    try {
        $env:CARGO_BUILD_BUILD_DIR = $wrapperTarget
        & cargo build --release --manifest-path $wrapperManifest --target-dir $wrapperTarget --offline
        $buildExitCode = $LASTEXITCODE
    } finally {
        if ($null -eq $prevBuildDir) {
            Remove-Item Env:CARGO_BUILD_BUILD_DIR -ErrorAction SilentlyContinue
        } else {
            $env:CARGO_BUILD_BUILD_DIR = $prevBuildDir
        }
    }
    if ($buildExitCode -ne 0 -or -not (Test-Path -LiteralPath $wrapperExe) -or -not (Test-Path -LiteralPath $linkWrapperExe)) {
        Write-Error "failed to build scripts/dx-capture-wrapper"
        exit 1
    }
}
$wrapperBuildTime = (Get-Item -LiteralPath $wrapperExe).LastWriteTimeUtc

# Route linker calls through dx-link-wrapper so:
# 1. unquoted paths with spaces in dx 0.7.10's @link_args.json (such as
#    gpui-pre's MSVC atlmfc lib path) are quoted before reaching the real linker,
# 2. dx.exe's DX_LINK=1 interception during fat/thin rustc compiles emits a
#    minimal COFF stub instead of running a redundant full link that collides
#    with thin hot-patch replay.
$env:DX_REAL_LINKER = $realLinkerPath
$env:DX_WRAPPER_NO_LINK = "1"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = $linkWrapperExe
$env:CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER = $linkWrapperExe

# dx 0.7.10 busts target/<triple>/<profile>/.fingerprint. This workspace uses
# Cargo's build-dir layout (target/<triple>/desktop-dev/build/<pkg>/<hash>/fingerprint),
# so a fresh tip binary never reruns rustc and the capture file is never written.
$packagesToBust = @("flowy-desktop")
$captureRoot = Join-Path $root "target\dx\.captured-args"
$latestCapture = $null
if (Test-Path $captureRoot) {
    $latestCapture = Get-ChildItem -LiteralPath $captureRoot -Directory |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
}
$memberPattern = [regex]::Match((Get-Content -LiteralPath (Join-Path $root "Cargo.toml") -Raw), '(?s)members\s*=\s*\[(.*?)\]')
if ($memberPattern.Success) {
    foreach ($memberMatch in [regex]::Matches($memberPattern.Groups[1].Value, '"([^"]+)"')) {
        $manifest = Join-Path $root (Join-Path $memberMatch.Groups[1].Value "Cargo.toml")
        if (-not (Test-Path -LiteralPath $manifest)) {
            continue
        }
        $nameMatch = [regex]::Match((Get-Content -LiteralPath $manifest -Raw), '(?m)^name\s*=\s*"([^"]+)"')
        if (-not $nameMatch.Success) {
            continue
        }
        $packageName = $nameMatch.Groups[1].Value
        if ($packageName -eq "flowy-desktop") {
            continue
        }
        $crateName = $packageName.Replace("-", "_")
        $capture = if ($null -ne $latestCapture) {
            Get-Item -LiteralPath (Join-Path $latestCapture.FullName "$crateName.lib.json") -ErrorAction SilentlyContinue
        }
        # Refresh old captures after wrapper updates so dx cannot replay a stale, oversized command line.
        if ($null -eq $capture -or $capture.LastWriteTimeUtc -lt $wrapperBuildTime) {
            $packagesToBust += $packageName
        }
    }
}

$fingerprintRoots = @(
    (Join-Path $root "target\$triple\desktop-dev"),
    (Join-Path $root "build.noindex\$triple\desktop-dev")
)
foreach ($fingerprintRoot in $fingerprintRoots) {
    $buildRoot = Join-Path $fingerprintRoot "build"
    $legacyRoot = Join-Path $fingerprintRoot ".fingerprint"
    foreach ($packageName in $packagesToBust) {
        $packageDir = Join-Path $buildRoot $packageName
        if (Test-Path -LiteralPath $packageDir) {
            Get-ChildItem -LiteralPath $packageDir -Directory | ForEach-Object {
                $fingerprint = Join-Path $_.FullName "fingerprint"
                if (Test-Path -LiteralPath $fingerprint) {
                    Remove-Item -LiteralPath $fingerprint -Recurse -Force
                }
            }
        }
        if (Test-Path -LiteralPath $legacyRoot) {
            Get-ChildItem -LiteralPath $legacyRoot -Directory | Where-Object {
                $_.Name -eq $packageName -or $_.Name.StartsWith("$packageName-")
            } | ForEach-Object {
                Remove-Item -LiteralPath $_.FullName -Recurse -Force
            }
        }
    }
}

# ~/.cargo/config.toml sets RUSTC_WRAPPER=sccache with force=true. This
# workspace config replaces that key with force=false, so the value below
# reaches dx instead of being overwritten. Do not pass it through --cargo-args:
# the nested quotes are split on the Windows command line and dx rejects them.
$env:RUSTC_WRAPPER = $wrapperExe

$dxArgs = @(
    "serve",
    $hotPatchFlag,
    "--hot-reload", "true",
    "--package", "flowy-desktop",
    "--bin", "flowy-app",
    "--platform", "windows",
    "--renderer", "native",
    "--fat-binary",
    "--open", "true"
)

& $dxPath @dxArgs
exit $LASTEXITCODE
