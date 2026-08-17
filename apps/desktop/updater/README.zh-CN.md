# Flowy 桌面端自动更新说明

应用内 OTA 的操作真源是根目录 [`BUILD_RELEASE.zh-CN.md`](../../../BUILD_RELEASE.zh-CN.md)
（ModelScope 分平台渠道）。本文只补充机制与本地命令；若与 `BUILD_RELEASE` 冲突，以
`BUILD_RELEASE` 为准。GitHub Releases 仍可用于**手动安装包**分发，见
[`RELEASING.zh-CN.md`](../../../RELEASING.zh-CN.md)。

## 工作方式

```text
正在运行的 App
  -> 请求构建时写入的平台专属 updater endpoint
  -> 下载 ModelScope 上的 allo/channels/{windows|macos|linux}/latest.json
  -> 判断是否有更高版本
  -> 下载 allo/{windows|macos|linux}/v{version}/ 下的更新包
  -> 用内置 pubkey 校验 .sig
  -> 安装并重启
```

正式发版必须叠加 updater + channel 配置，例如：

```text
--config apps/desktop/tauri.updater.conf.json
--config apps/desktop/tauri.channel.windows.conf.json
```

三端 endpoint：

```text
.../FilePath=allo/channels/windows/latest.json
.../FilePath=allo/channels/macos/latest.json
.../FilePath=allo/channels/linux/latest.json
```

旧的共享渠道 `allo/channels/alpha/latest.json` **已废弃**。仍指向 alpha 的旧安装包
不会收到新渠道更新，需要安装带 channel overlay 的新包。

公钥 keyID：`6FD07533C4187B64`（内嵌于 `tauri.conf.json`）。

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

仓库内置：

- `apps/desktop/tauri.updater.conf.json` — 产出 `.sig`
- `apps/desktop/tauri.channel.{windows|macos|linux}.conf.json` — 写入平台 endpoint

**务必传文件路径，不要内联 JSON**：Windows PowerShell 5.1 会剥掉内联 `--config '{...}'`
里的双引号。

macOS：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:mac arm --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.macos.conf.json
bun run make:latest --host modelscope --channel macos --collect
bun run upload:modelscope -- --channel macos
```

Windows：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""

bun run build:win --signed --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.windows.conf.json
bun run make:latest --host modelscope --channel windows --collect
bun run upload:modelscope -- --channel windows
```

Linux：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:linux --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.linux.conf.json
bun run make:latest --host modelscope --channel linux --collect
bun run upload:modelscope -- --channel linux
```

每个渠道维护自己的 `version`，三端可以长期不同步。

## latest.json 与 channel.yml

```bash
bun run make:latest --host modelscope --channel windows --collect
```

会写入 `apps/desktop/updater/latest.windows.json`（macOS/linux 同理），并在
`--collect` 时同步 `channel.yml` 与 `dist/desktop/`。上传：

```bash
bun run upload:modelscope -- --channel windows
```

同渠道、同版本补架构条目时可用：

```bash
bun run upload:modelscope -- --channel windows --merge-remote
```

（不会合并其他 OS 渠道。）

## 验证

1. ModelScope 上确认 `allo/{windows|macos|linux}/v{version}/` 有包与 `.sig`。
2. 确认对应 `allo/channels/{platform}/latest.json` 的 `version` 与 URL 正确。
3. 安装**带 channel overlay 的新包**后检查更新。
