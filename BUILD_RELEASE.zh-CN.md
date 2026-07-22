# Flowy 桌面端构建与发版手册（ModelScope OTA）

本文记录 **allo** 桌面端当前的构建与自动更新发版流程。应用内 OTA 已切换至 ModelScope；GitHub Releases 相关的一键脚本（`release:mac` / `release:win`）仍可用于手动安装包分发，详见 `RELEASING.zh-CN.md`。

## 概览

| 项目 | 说明 |
|------|------|
| 版本号真源 | 根目录 `Cargo.toml` → `[workspace.package].version` |
| OTA 渠道 | `alpha` |
| ModelScope 仓库 | [flowy2025/flowyaipc](https://www.modelscope.cn/models/flowy2025/flowyaipc/tree/master/allo) |
| 客户端拉取端点 | `allo/channels/alpha/latest.json` |
| Updater 公钥 keyID | `8600581EC8FDE447`（内嵌于 `apps/desktop/tauri.conf.json`） |

### ModelScope 目录结构

```text
allo/
├── alpha.yml                          # 渠道指针（版本元数据）
├── channels/alpha/latest.json         # Tauri updater 清单（客户端实际请求）
└── v{version}/                        # 各平台签名更新包 + 对应 .sig
    ├── Flowy_{version}_x64-setup.exe
    ├── Flowy_{version}_aarch64-setup.exe
    ├── Flowy_{version}_universal.app.tar.gz
    └── Flowy_{version}_x86_64.AppImage
```

### 两类产物

- **手动安装包**：`.dmg`、`.exe`、`.msi`、`.AppImage`、`.deb`、`.rpm` 等，供用户自行下载安装。
- **自动更新产物**：Tauri updater 可安装的包（Windows NSIS `.exe`、macOS `.app.tar.gz`、Linux `.AppImage`）及其 `.sig`，加上合并后的 `latest.json`。

> Tauri updater 签名（minisign）与系统代码签名（macOS Developer ID / Windows Authenticode）是两套机制。前者保证 OTA 包未被篡改；后者影响 Gatekeeper、SmartScreen 等系统信任提示。

---

## 一次性环境准备

### 1. 构建工具

- **Rust** + **Bun**（仓库使用 `bun` 驱动脚本）
- **Tauri CLI**：`bun install` 后会带上 `@tauri-apps/cli`
- **Python 3** + `pip install modelscope`（仅上传步骤需要）

### 2. Updater 签名私钥

私钥路径：`apps/desktop/signing/nomifun-updater.key`（已被 gitignore，需从团队密钥库拷贝）。

必须与 `apps/desktop/tauri.conf.json` 内嵌的 `pubkey` 匹配（keyID `8600581EC8FDE447`）。**更换公钥后，已安装旧公钥的客户端需手动重装一次。**

### 3. ModelScope Token

```bash
cp apps/desktop/signing/.env.modelscope.example apps/desktop/signing/.env.modelscope
# 编辑 .env.modelscope，填入 MODELSCOPE_TOKEN（https://www.modelscope.cn/my/myaccesstoken）
```

也可直接导出环境变量：

```bash
export MODELSCOPE_TOKEN="ms-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
```

### 4. Windows 构建注意

若 Cargo 拉依赖时报 `CRYPT_E_REVOCATION_OFFLINE`，仓库已在 `.cargo/config.toml` 与 `scripts/desktop-build-win.ps1` 中设置 `check-revoke = false` / `CARGO_HTTP_CHECK_REVOKE=false`。

叠加 updater 配置时**务必传文件路径**，不要内联 JSON（PowerShell 5.1 会剥掉内联 `--config '{...}'` 的双引号）：

```text
apps/desktop/tauri.updater.conf.json   # {"bundle":{"createUpdaterArtifacts":true}}
```

---

## 发版前： bump 版本号

```bash
bun run bump 0.2.15          # 同步 Cargo.toml、Cargo.lock、package.json、ui/package.json
bun run bump 0.2.15 --tag    # 额外提交并打 tag v0.2.15（工作区须干净）
```

---

## 标准发版流程（分平台构建 + ModelScope 上传）

各平台**不能交叉编译**，需在对应系统上分别构建。典型顺序：macOS → Windows → Linux（或任意顺序，最终合并 `latest.json`）。

### 步骤 1：构建带签名的 updater 产物

**macOS**

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:mac --config apps/desktop/tauri.updater.conf.json
```

公开分发时建议加 `--signed`（Developer ID 签名 + 公证），见 `RELEASING.zh-CN.md`。

**Windows**

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""

bun run build:win --config apps/desktop/tauri.updater.conf.json
# 有 Authenticode 证书时：
# bun run build:win --signed --config apps/desktop/tauri.updater.conf.json
# 或：bun run release:win（指纹在环境变量 / .env.release 时自动 --signed）
```

**Linux**

```bash
# 依赖（Debian/Ubuntu）
sudo apt-get install -y pkg-config libgbm-dev libayatana-appindicator3-dev librsvg2-dev

export TAURI_SIGNING_PRIVATE_KEY="$(cat apps/desktop/signing/nomifun-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

bun run build:linux --config apps/desktop/tauri.updater.conf.json
```

### 步骤 2：生成 ModelScope 版 latest.json 并收集产物

在**当前构建机**上执行（`--collect` 会把 updater 包、`.sig`、`latest.json` 复制到 `dist/desktop/`）：

```bash
bun run make:latest --host modelscope --channel alpha --collect
```

可选参数：

```bash
bun run make:latest --host modelscope --channel alpha --collect \
  --version 0.2.15 \
  --notes "修复若干问题"

bun run make:latest --host modelscope --channel alpha --collect \
  --notes-file release-notes.md
```

`make:latest` 会扫描 `target/**/release/bundle/` 下的 updater 产物，将本机平台条目写入 `apps/desktop/updater/latest.json`，并保留已有其他平台条目。

### 步骤 3：上传到 ModelScope

**首台机器（仅本机构建了部分平台）**

```bash
bun run upload:modelscope
```

等价于：

```bash
python scripts/upload-modelscope-release.py \
  --repo flowy2025/flowyaipc \
  --prefix allo \
  --channel alpha \
  --dist-dir dist/desktop/
```

**后续机器（合并远端已有平台条目）**

在另一台平台构建并执行 `make:latest --collect` 后：

```bash
bun run upload:modelscope -- --merge-remote
```

`--merge-remote` 会从 ModelScope 拉取当前 `latest.json`，把远端已有、本机未构建的平台条目合并进来，再上传完整清单。

### 步骤 4：验证

1. 浏览器打开 ModelScope 文件页，确认 `allo/v{version}/` 下有对应安装包与 `.sig`。
2. 确认 `allo/channels/alpha/latest.json` 已更新，`version` 与各 `platforms` 条目 URL 指向 ModelScope。
3. 在**已安装的旧版本**客户端中点击标题栏右上角更新按钮，或打开设置 → 检查更新，确认能检测到新版本并完成安装。

---

## Windows 一键示例（v0.2.14）

已在 Windows 上验证通过的完整命令：

```powershell
# 1. bump（若尚未改版本）
bun run bump 0.2.14

# 2. 构建
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content apps/desktop/signing/nomifun-updater.key -Raw
bun run build:win --config apps/desktop/tauri.updater.conf.json

# 3. 生成清单并收集到 dist/desktop/
bun run make:latest --host modelscope --channel alpha --collect

# 4. 上传
bun run upload:modelscope
```

---

## 多平台合并发版

```text
Mac 构建机:
  build:mac --config tauri.updater.conf.json
  make:latest --host modelscope --channel alpha --collect
  upload:modelscope

Windows 构建机:
  git pull   # 拿到最新 latest.json（若已提交回仓库）
  build:win --config tauri.updater.conf.json
  make:latest --host modelscope --channel alpha --collect
  upload:modelscope -- --merge-remote

Linux 构建机:
  build:linux --config tauri.updater.conf.json
  make:latest --host modelscope --channel alpha --collect
  upload:modelscope -- --merge-remote
```

上传脚本会自动过滤本机 `dist/desktop/` 中不存在的平台条目，避免覆盖未构建的平台。

---

## 常用脚本速查

| 命令 | 用途 |
|------|------|
| `bun run build:win` / `build:mac` / `build:linux` | 打当前平台安装包 → `dist/desktop/` |
| `bun run build:updater` | 快捷：带 `createUpdaterArtifacts` 的 tauri build |
| `bun run make:latest --host modelscope --channel alpha --collect` | 生成 ModelScope URL 的 `latest.json` 并收集产物 |
| `bun run upload:modelscope` | 上传 `dist/desktop/` 到 ModelScope |
| `bun run upload:modelscope -- --merge-remote` | 合并远端平台后上传 |
| `bun run upload:modelscope -- --dry-run` | 只打印上传计划，不实际上传 |
| `bun run bump <version>` | 统一改版本号 |
| `bun run dev` | 本地开发（可测检查更新，**不能**完整测安装/重启） |

---

## 上传脚本参数

`scripts/upload-modelscope-release.py`：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--repo` | `flowy2025/flowyaipc` | ModelScope 模型仓库 |
| `--prefix` | `allo` | 仓库内路径前缀 |
| `--channel` | `alpha` | 渠道子目录 |
| `--dist-dir` | （必填） | 含 `latest.json` 与签名产物的目录，通常为 `dist/desktop/` |
| `--env-file` | `apps/desktop/signing/.env.modelscope` | Token 配置文件 |
| `--merge-remote` | — | 合并远端 `latest.json` 中缺失的平台 |
| `--dry-run` | — | 预检，不上传 |

---

## CI 发版（可选）

工作流：`.github/workflows/release-modelscope.yml`

- 触发：`workflow_dispatch`（手动输入版本）或推送 `v*` tag
- 需要 GitHub Secret：`MODELSCOPE_TOKEN`
- 各平台构建 job 需产出 `dist/desktop/latest.json` 及对应 updater 包，作为 artifact `allo-updater-*` 上传后由 `modelscope-upload` job 合并并调用上传脚本

---

## OTA 测试说明

| 场景 | 做法 |
|------|------|
| 开发模式测检查更新 | `bun run dev`，可验证能否拉到 ModelScope 清单 |
| 测完整安装流程 | 安装**旧版 release 包**（如 0.2.13），再触发更新到当前已发布版本 |
| 模拟旧版本 | `bun run bump 0.2.13` 后重启 dev（仅版本号回退，非真实旧包） |

客户端 updater 端点配置于 `apps/desktop/tauri.conf.json`：

```text
https://modelscope.cn/api/v1/models/flowy2025/flowyaipc/repo?Revision=master&FilePath=allo/channels/alpha/latest.json
```

---

## 故障排查

| 现象 | 处理 |
|------|------|
| `MODELSCOPE_TOKEN not set` | 配置 `.env.modelscope` 或导出环境变量 |
| `modelscope package not installed` | `pip install modelscope` |
| `no updater artifacts found` | 确认用了 `tauri.updater.conf.json` 构建，且执行了 `make:latest --collect` |
| `no uploadable platform entries remain` | `dist/desktop/` 中缺少 `latest.json` 引用的安装包文件名 |
| 客户端检查更新失败 | 确认 ModelScope 上 `latest.json` 可访问；公钥与构建私钥匹配 |
| Windows Cargo 证书吊销离线 | 确认 `CARGO_HTTP_CHECK_REVOKE=false` 已生效 |
| PowerShell 内联 JSON 报错 | 改用 `--config apps/desktop/tauri.updater.conf.json` 文件路径 |

---

## 相关文件

| 路径 | 说明 |
|------|------|
| `apps/desktop/tauri.conf.json` | 生产配置（updater endpoint + pubkey） |
| `apps/desktop/tauri.dev.conf.json` | 开发配置（继承 ModelScope endpoint） |
| `apps/desktop/tauri.updater.conf.json` | 叠加配置，启用 `createUpdaterArtifacts` |
| `apps/desktop/updater/latest.json` | 本地维护的清单模板（上传前由 `make:latest` 更新） |
| `scripts/make-latest-json.mjs` | 生成 / 合并 `latest.json` |
| `scripts/upload-modelscope-release.py` | ModelScope 上传 |
| `scripts/run-upload-modelscope.mjs` | 跨平台 Python 启动器 |
| `apps/desktop/signing/.env.modelscope.example` | Token 配置模板 |

更详细的 updater 机制说明见 `apps/desktop/updater/README.zh-CN.md`（以本文为准）。
