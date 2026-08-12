# Flowy 桌面端构建与发版手册（ModelScope OTA）

本文记录 **allo** 桌面端当前的构建与自动更新发版流程。应用内 OTA 使用 ModelScope
**分平台独立渠道**；GitHub Releases 相关的一键脚本（`release:mac` / `release:win`）
仍可用于手动安装包分发，详见 `RELEASING.zh-CN.md`。

## 概览

| 项目 | 说明 |
|------|------|
| 版本号真源 | 根目录 `Cargo.toml` → `[workspace.package].version`（打在本 commit 上） |
| OTA 渠道 | `windows` / `macos` / `linux`（各自一份 `latest.json`，版本可长期不同步） |
| ModelScope 仓库 | [flowy2025/flowyaipc](https://www.modelscope.cn/models/flowy2025/flowyaipc/tree/master/allo) |
| 客户端拉取端点 | `allo/channels/{windows\|macos\|linux}/latest.json`（构建时用 channel overlay 写入） |
| Updater 公钥 keyID | `6FD07533C4187B64`（内嵌于 `apps/desktop/tauri.conf.json`） |

旧的共享渠道 `allo/channels/alpha/latest.json` **已废弃**，新安装包不再读取。

### ModelScope 目录结构

```text
allo/
├── channels/windows/latest.json       # Windows 专属清单（独立 version）
├── channels/macos/latest.json         # macOS 专属清单
├── channels/linux/latest.json         # Linux 专属清单
├── channels/{platform}/channel.yml    # 渠道指针（信息用）
├── windows/v{version}/                # Windows 签名更新包 + .sig
├── macos/v{version}/                  # macOS 签名更新包 + .sig
└── linux/v{version}/                  # Linux 签名更新包 + .sig
```

### 两类产物

- **手动安装包**：`.dmg`、`.exe`、`.msi`、`.AppImage`、`.deb`、`.rpm` 等。
- **自动更新产物**：Tauri updater 可安装的包及其 `.sig`，加上该平台渠道的 `latest.json`。

> Tauri updater 签名（minisign）与系统代码签名（macOS Developer ID / Windows Authenticode）是两套机制。

---

## 一次性环境准备

### 1. 构建工具

- **Rust** + **Bun**
- **Tauri CLI**：`bun install` 后会带上 `@tauri-apps/cli`
- **Python 3** + `pip install modelscope`（仅上传步骤需要）

### 2. Updater 签名私钥

私钥路径：`apps/desktop/signing/nomifun-updater.key`（已被 gitignore）。

必须与 `apps/desktop/tauri.conf.json` 内嵌的 `pubkey` 匹配（keyID `6FD07533C4187B64`）。

### 3. ModelScope Token

```bash
cp apps/desktop/signing/.env.modelscope.example apps/desktop/signing/.env.modelscope
# 编辑 .env.modelscope，填入 MODELSCOPE_TOKEN
```

### 4. Windows 构建注意

叠加配置时**务必传文件路径**，不要内联 JSON：

```text
apps/desktop/tauri.updater.conf.json
apps/desktop/tauri.channel.windows.conf.json   # 写入 Windows endpoint
```

---

## 发版前： bump 版本号

```bash
bun run bump 0.4.2
```

三端版本可不同：在 **不同 commit** 上 bump 到不同版本，再打对应平台 tag。

---

## 标准发版流程（单平台）

各平台**不能交叉编译**。每次只发一个平台渠道。

### 步骤 1：构建（必须叠加 channel config）

**macOS**

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:mac \
  --config apps/desktop/tauri.updater.conf.json \
  --config apps/desktop/tauri.channel.macos.conf.json
```

**Windows**

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""

bun run build:win `
  --config apps/desktop/tauri.updater.conf.json `
  --config apps/desktop/tauri.channel.windows.conf.json
```

**Linux**

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:linux \
  --config apps/desktop/tauri.updater.conf.json \
  --config apps/desktop/tauri.channel.linux.conf.json
```

### 步骤 2：生成该平台清单并收集

```bash
bun run make:latest --host modelscope --channel windows --collect   # 或 macos / linux
```

会写入 `apps/desktop/updater/latest.{channel}.json`，并把产物拷到 `dist/desktop/`。

### 步骤 3：上传

```bash
bun run upload:modelscope -- --channel windows
```

同渠道、同版本补架构条目时可用 `--merge-remote`（**不会**合并其他 OS 渠道）。

### 步骤 4：验证

1. 确认 `allo/{platform}/v{version}/` 下有包与 `.sig`。
2. 确认 `allo/channels/{platform}/latest.json` 的 `version` 正确。
3. 用**带 channel overlay 的新安装包**测检查更新。

---

## Windows 一键示例

```powershell
bun run bump 0.4.2
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
bun run build:win --config apps/desktop/tauri.updater.conf.json --config apps/desktop/tauri.channel.windows.conf.json
bun run make:latest --host modelscope --channel windows --collect
bun run upload:modelscope -- --channel windows
```

---

## 三端独立发版（版本可分叉）

```text
Windows 0.4.2:
  bump 0.4.2 → commit → tag v0.4.2-windows → push tag
  → 只更新 channels/windows/latest.json

一周后 macOS 0.1.8:
  bump 0.1.8 → commit → tag v0.1.8-macos → push tag
  → 只更新 channels/macos/latest.json（windows 清单不受影响）
```

---

## 常用脚本速查

| 命令 | 用途 |
|------|------|
| `bun run build:win` / `build:mac` / `build:linux` | 打当前平台安装包 |
| `... --config tauri.updater.conf.json --config tauri.channel.<p>.conf.json` | 正式 OTA 构建 |
| `bun run make:latest --host modelscope --channel <p> --collect` | 生成该渠道清单并收集产物 |
| `bun run upload:modelscope -- --channel <p>` | 上传到对应渠道 |
| `bun run upload:modelscope -- --channel <p> --merge-remote` | 同渠道同版本合并远端缺失键 |
| `bun run bump <version>` | 统一改版本号 |

---

## 上传脚本参数

`scripts/upload-modelscope-release.py`：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--repo` | `flowy2025/flowyaipc` | ModelScope 模型仓库 |
| `--prefix` | `allo` | 仓库内路径前缀 |
| `--channel` | 从清单推断 | `windows` \| `macos` \| `linux` |
| `--dist-dir` | （必填） | 含 `latest.json` 与签名产物的目录 |
| `--merge-remote` | — | 仅合并同渠道、同版本的缺失平台键 |
| `--dry-run` | — | 预检，不上传 |

---

## CI 发版

工作流：`.github/workflows/release-modelscope.yml`。

| tag | 渠道 | 发布平台键 |
|------|------|-----------|
| `v0.4.2-windows` | `windows` | `windows-x86_64` |
| `v0.1.8-macos` | `macos` | `darwin-x86_64`、`darwin-aarch64` |
| `v0.3.1-linux` | `linux` | `linux-x86_64` |

tag 中的版本必须与**该 tag 指向 commit** 的 `Cargo.toml` version 一致。

三端可并行（concurrency 按 tag ref 分组）。构建命令叠加对应
`tauri.channel.*.conf.json`，上传到独立渠道清单。

```bash
bun run bump 0.4.2
git add Cargo.toml Cargo.lock package.json ui/package.json
git commit -m "chore(release): v0.4.2"
git push origin HEAD
git tag v0.4.2-windows
git push origin v0.4.2-windows
# macOS / Linux 可在其他版本 commit 上另行打 tag，无需等待
```

Secrets：`TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（可选）、
`MODELSCOPE_TOKEN`（推荐放在 `modelscope-alpha` environment）。

---

## OTA 测试说明

| 场景 | 做法 |
|------|------|
| 开发模式测检查更新 | `bun run dev`（默认读 windows endpoint；完整测需正式包） |
| 测完整安装流程 | 安装带 channel overlay 的旧版包，再发更高版本到该渠道 |
| 旧 alpha 客户端 | 不会收到新渠道更新，需重装 |

客户端正式 endpoint 由 channel overlay 写入，例如 Windows：

```text
.../FilePath=allo/channels/windows/latest.json
```

---

## 故障排查

| 现象 | 处理 |
|------|------|
| `MODELSCOPE_TOKEN not set` | 配置 `.env.modelscope` 或导出环境变量 |
| `--channel 必须是 windows\|macos\|linux` | 不要再传 `alpha` |
| `no updater artifacts found` | 确认用了 updater + channel 两份 `--config`，并 `make:latest --collect` |
| 客户端检查更新失败 | 确认安装包 endpoint 指向正确渠道；公钥匹配 |
| PowerShell 内联 JSON 报错 | 改用配置文件路径 |

---

## 相关文件

| 路径 | 说明 |
|------|------|
| `apps/desktop/tauri.conf.json` | 生产配置（默认 windows endpoint + pubkey） |
| `apps/desktop/tauri.channel.*.conf.json` | 平台专属 updater endpoint |
| `apps/desktop/tauri.updater.conf.json` | 启用 `createUpdaterArtifacts` |
| `apps/desktop/updater/latest.{platform}.json` | 本机维护的分渠道清单 |
| `scripts/make-latest-json.mjs` | 生成分渠道 `latest.json` |
| `scripts/upload-modelscope-release.py` | ModelScope 上传 |
| `scripts/verify-modelscope-release.py` | 发布后校验 |

更详细的 updater 机制说明见 `apps/desktop/updater/README.zh-CN.md`（以本文为准）。
