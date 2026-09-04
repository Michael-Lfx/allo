# Agent Store CLI（cli/ 目录）

把 Agent Store 的 **Rust 核心后端** 与 **@web 前端 UI**（`web/`，即
`allo-app-server-web`，App Server 协议的消费方）打包为**单个可执行文件**
`agent-store.exe`。用户通过本目录即可启动「前后端一体」的服务——后端 API 与
内嵌 Web UI 在**同一端口**服务，浏览器零配置连接（UI 默认 WS 地址
`ws://127.0.0.1:8787/api/app-server/ws` 与该 host 默认端口一致，无需改设置）。

```text
cli/
  start.cmd         ← 双击或命令行启动（Windows）
  agent-store.exe   ← 构建产物（由 bun run agent-store:build 生成，不入库）
```

## 快速开始

```bat
bun run agent-store:build   :: 构建前端 web/dist + release exe，复制到 cli\
cli\start.cmd               :: 启动并自动打开浏览器 http://127.0.0.1:8787
```

## 常用参数（透传给 agent-store.exe）

| 参数 | 说明 | 默认 |
|---|---|---|
| `--port <n>` | 监听端口（API 与内嵌 Web UI 同源，前端按页面地址自动连接，换端口零配置） | `8787` |
| `--host <ip>` | 监听地址；只有明确需要局域网访问才改成 `0.0.0.0` | `127.0.0.1` |
| `--data-dir <dir>` | 后端数据目录（db/存储）；默认与桌面端/`serve:web` 共享同一份状态 | 每用户 Flowy/Nomi 目录 |
| `--auth` | 开启登录模式（默认本地可信模式，免登录） | 关闭 |
| `--no-open` | 不自动打开浏览器 | 打开 |

环境变量：`NOMIFUN_DATA_DIR` / `FLOWY_DATA_DIR`（数据目录最终值）、
`NOMIFUN_ADMIN_USERNAME` / `NOMIFUN_ADMIN_PASSWORD`（`--auth` 模式下首启建号）。

## 构建说明

```text
bun run agent-store:build
  = 1) bun run --cwd ./web build          → web/dist（前端产物）
    2) cargo build --release -p agent-store --features static-webui
                                           → target/release/agent-store.exe
    3) bun scripts/copy-agent-store-cli.mjs → 复制到 cli/agent-store.exe
```

- release 构建会把 `web/dist` **编译进二进制的内嵌包**（`rust-embed`）；
- debug 构建（`cargo run -p agent-store --features static-webui`）从磁盘读取
  `web/dist`，便于前端迭代；
- 未启用 `static-webui` feature 的构建是 API-only，SPA 回退返回 404；
- 仓库的 `cargo check --workspace` 默认不带该 feature，因此没有 `web/dist`
  的新克隆也能编译（前端产物被 gitignore）。

## 数据目录与并发说明

- 默认数据目录与其他 host（桌面应用、`bun run serve:web`）相同，因此能看到
  同一份 Agent Store 状态；后端对该目录持有**独占锁**
  （`{data_dir}/server.lock`）——桌面应用正在运行时再启动本服务会快速失败，
  这是防双写保护，不是故障。
- 端口 8787 被占用时同样快速失败并提示；请关闭其他实例或换端口。

## 与 docs/agent-store/ 的关系

本产物遵循 `docs/agent-store/00-architecture-decision.md`：
Rust 核心走 `nomifun-app` 完整引导（Catalog/Importer/Marketplace/OAuth/App
Server 全挂载），`web/` 是 App Server 协议的消费方；两个二进制统一为一个
（本目录入口）；默认本地可信进程模型（LocalPrincipal/AuthContext 由主进程
建立），凭据仍只走本地安全存储。