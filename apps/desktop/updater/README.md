# Flowy desktop auto-update

Operational source of truth for in-app OTA is root
[`BUILD_RELEASE.zh-CN.md`](../../../BUILD_RELEASE.zh-CN.md) (ModelScope
channel). If this file disagrees with `BUILD_RELEASE`, follow
`BUILD_RELEASE`. GitHub Releases remain for **manual installer** distribution;
see [`RELEASING.md`](../../../RELEASING.md).

## How it works

```text
Running app
  -> fetch updater endpoint from apps/desktop/tauri.conf.json
  -> download ModelScope allo/channels/alpha/latest.json
  -> compare versions
  -> download platform package
  -> verify .sig with embedded pubkey
  -> install and restart
```

Endpoint:

```text
https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo?Revision=master&FilePath=allo/channels/alpha/latest.json
```

Pubkey keyID: `8600581EC8FDE447`.

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

Use file-path `--config apps/desktop/tauri.updater.conf.json` (never inline JSON
on Windows PowerShell 5.1). Private key:

```text
apps/desktop/signing/nomifun-updater.key
```

```bash
# macOS / Linux
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
bun run build:mac --config apps/desktop/tauri.updater.conf.json   # or build:linux
bun run make:latest --host modelscope --channel alpha --collect
bun run upload:modelscope
```

```powershell
# Windows — Authenticode when WINDOWS_CERTIFICATE_THUMBPRINT is set
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
$env:WINDOWS_CERTIFICATE_THUMBPRINT = "A1B2C3..."
bun run build:win --signed --config apps/desktop/tauri.updater.conf.json
bun run make:latest --host modelscope --channel alpha --collect
bun run upload:modelscope
```

`release:win` enables `--signed` automatically when the thumbprint is available
(env or `.env.release`).

## OS trust vs updater trust

Updater minisign ≠ macOS Developer ID / Windows Authenticode. Without OS code
signing, OTA still verifies packages, but manual installers may show
Gatekeeper / SmartScreen warnings.
