# Flowy 桌面端自动更新说明

应用内 OTA 的操作真源是根目录 [`BUILD_RELEASE.zh-CN.md`](../../../BUILD_RELEASE.zh-CN.md)
（ModelScope 渠道）。本文只补充机制与本地命令；若与 `BUILD_RELEASE` 冲突，以
`BUILD_RELEASE` 为准。GitHub Releases 仍可用于**手动安装包**分发，见
[`RELEASING.zh-CN.md`](../../../RELEASING.zh-CN.md)。

## 工作方式

```text
正在运行的 App
  -> 请求 apps/desktop/tauri.conf.json 里的 updater endpoint
  -> 下载 ModelScope 上的 allo/channels/alpha/latest.json
  -> 判断是否有更高版本
  -> 下载当前平台对应的更新包
  -> 用内置 pubkey 校验 .sig
  -> 安装并重启
```

当前 endpoint：

```text
https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo?Revision=master&FilePath=allo/channels/alpha/latest.json
```

公钥 keyID：`8600581EC8FDE447`（内嵌于 `tauri.conf.json`）。

## 密钥区别

自动更新使用一把 Tauri updater 私钥：

```text
apps/desktop/signing/nomifun-updater.key
```

发版时把私钥内容写入环境变量：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
```

这把密钥只负责 updater 验签，不负责系统信任：

- macOS Gatekeeper 仍需要 Developer ID 签名和公证。
- Windows SmartScreen / 未知发布者仍需要 Authenticode 签名。
- 没有 OS 代码签名时，自动更新验签仍可工作，但手动安装体验不够可信。

## 产物命名（强制）

`productName` 为 `Flowy`。updater 清单只接受 `Flowy` 前缀产物（`make:latest`
会拒绝遗留的 `NomiFun_*` 文件名）：

| 平台键 | 典型 updater 包 |
| --- | --- |
| `windows-x86_64` | `Flowy_{version}_x64-setup.exe` |
| `windows-aarch64` | `Flowy_{version}_aarch64-setup.exe` |
| `darwin-x86_64` / `darwin-aarch64` | `Flowy.app.tar.gz` 或 `Flowy_{version}_universal.app.tar.gz` |
| `linux-x86_64` | `Flowy_{version}_amd64.AppImage` 或 `Flowy_{version}_x86_64.AppImage` |
| `linux-aarch64` | `Flowy_{version}_aarch64.AppImage` 或 `Flowy_{version}_arm64.AppImage` |

## 构建自动更新产物

仓库内置叠加配置 `apps/desktop/tauri.updater.conf.json`（
`{"bundle":{"createUpdaterArtifacts":true}}`），用 `--config` 叠加即可产出 `.sig`。
**务必传文件路径，不要内联 JSON**：Windows PowerShell 5.1 会剥掉内联 `--config '{...}'`
里的双引号。

macOS：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:mac --config apps/desktop/tauri.updater.conf.json
bun run make:latest --host modelscope --channel alpha --collect
```

Windows（有 Authenticode 时加 `--signed`；`release:win` 在指纹可用时会自动带上）：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
$env:WINDOWS_CERTIFICATE_THUMBPRINT = "A1B2C3..."   # 可选；有则启用 Authenticode

bun run build:win --signed --config apps/desktop/tauri.updater.conf.json
bun run make:latest --host modelscope --channel alpha --collect
```

Linux：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:linux --config apps/desktop/tauri.updater.conf.json
bun run make:latest --host modelscope --channel alpha --collect
```

Linux 同时产出 `.AppImage` / `.deb` / `.rpm` 时，清单 updater 条目选用
`.AppImage`；其余可作为手动安装包上传。

## latest.json 与 alpha.yml

```bash
bun run make:latest --host modelscope --channel alpha --collect
```

会写入 `apps/desktop/updater/latest.json`，并在 `--collect` 时同步
`apps/desktop/updater/alpha.yml` 与 `dist/desktop/`。上传：

```bash
bun run upload:modelscope
# 后续平台合并远端条目：
bun run upload:modelscope -- --merge-remote
```

## 验证

1. ModelScope 上确认 `allo/v{version}/` 有包与 `.sig`，且
   `allo/channels/alpha/latest.json` 的 `platforms` URL 指向 ModelScope。
2. 旧版客户端检查更新，确认能检测到并安装。
