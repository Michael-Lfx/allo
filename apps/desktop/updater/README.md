# Flowy desktop auto-update

Operational source of truth for in-app OTA is root
[`BUILD_RELEASE.zh-CN.md`](../../../BUILD_RELEASE.zh-CN.md) (ModelScope
channels). If this file disagrees with `BUILD_RELEASE`, follow
`BUILD_RELEASE`. GitHub Releases remain for **manual installer** distribution;
see [`RELEASING.md`](../../../RELEASING.md).

## How it works

```text
Running app
  -> fetch platform-specific updater endpoint (baked in at build time)
  -> download ModelScope allo/channels/{windows|macos|linux}/latest.json
  -> compare versions
  -> download package under allo/{windows|macos|linux}/v{version}/
  -> verify .sig with embedded pubkey
  -> install and restart
```

Release builds must overlay both updater + channel configs, for example:

```text
--config apps/desktop/tauri.updater.conf.json
--config apps/desktop/tauri.channel.windows.conf.json
```

Endpoints:

```text
.../FilePath=allo/channels/windows/latest.json
.../FilePath=allo/channels/macos/latest.json
.../FilePath=allo/channels/linux/latest.json
```

The legacy shared `allo/channels/alpha/latest.json` channel is deprecated.
Existing installs that still point at alpha will not receive new-channel
updates; users need a fresh installer built with a channel overlay.

Pubkey keyID: `6FD07533C4187B64`.

## Naming (enforced)

`productName` is `Flowy`. `make:latest` rejects legacy `NomiFun_*` artifact
names. Expected updater packages:

| Platform key | Typical package |
| --- | --- |
| `windows-x86_64` | `Flowy_{version}_x64-setup.exe` |
| `windows-aarch64` | `Flowy_{version}_aarch64-setup.exe` |
| `darwin-*` | `Flowy.app.tar.gz` or `Flowy_{version}_universal.app.tar.gz` |
| `linux-x86_64` | `Flowy_{version}_amd64.AppImage` or `Flowy_{version}_x86_64.AppImage` |
| `linux-aarch64` | `Flowy_{version}_aarch64.AppImage` or `Flowy_{version}_arm64.AppImage` |

## Build updater artifacts

```bash
# macOS
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
bun run build:mac universal --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.macos.conf.json
bun run make:latest --host modelscope --channel macos --collect
bun run upload:modelscope -- --channel macos
```

```powershell
# Windows
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
bun run build:win x64 arm64 --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.windows.conf.json
bun run make:latest --host modelscope --channel windows --collect
bun run upload:modelscope -- --channel windows
```

```bash
# Linux
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
bun run build:linux --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.linux.conf.json
bun run make:latest --host modelscope --channel linux --collect
bun run upload:modelscope -- --channel linux
```

Each channel keeps its own `version`. Windows/macOS/Linux may diverge.

## OS trust vs updater trust

Updater minisign ≠ macOS Developer ID / Windows Authenticode. Without OS code
signing, OTA still verifies packages, but manual installers may show
Gatekeeper / SmartScreen warnings.
