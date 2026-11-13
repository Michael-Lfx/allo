| Crate | 职责 | 对外 |
| ----------- | ------------------------- | ------------------ |
| `flowy-net` | 代理 + Client 工厂 | 传输钩子 |
| `flowy-ota` | 镜像、清单、rollout、验签、下载缓存槽、各平台安装 | `Updater` + `DownloadedUpdateCache` |
| `flowy-obs` | 日志、Sentry 崩溃上报、遥测 allowlist/outbox | `init_tracing` / `Telemetry` |
| `flowy-core` | 文档图模型、历史、原子写、`DocumentSession` | 会话命令 API |
| `flowy-auth` | 云登录 Email OTP + JWT 会话落盘；经 `flowy-net` 调 Flowy `/user/*` | `AuthClient` / `SessionStore` / `Session`；env：`FLOWY_API_BASE_URL` / `FLOWY_AUTH_CHANNEL` / `FLOWY_AUTH_APP` |
| `flowy-chat` | 任务线程投影 + mock stream | `Thread` / `Item` / `Beat` / `MockStream` |
| `flowy-ui` | 主题色板、Accent/Density | `install` + token fns |
| `flowy-shell` | deep link / tray / notify / autostart | `install(Brand, open_main)` / `configure_brand` |
| `flowy-agent` | UI↔dsh 端口；多 session / 流式投影 / sidecar 标题；`Model`/`Plan`/`FlexProvider` | `AgentHandle` / `ChatLine` / `TurnStatus`；env：`FLOWY_LLM_API_KEY` / `FLOWY_LLM_BASE_URL` / `FLOWY_LLM_MODEL`；Flex BYOK：`LlmConfig::from_flex` + `apply_config` |
| `flowy-i18n` | 12 locales + `{product}` | `I18n` / `Locale` |
| `flowy-studio` | Home / Chat / Automation Entity | `StudioPanel` / `StudioEvent`（含 `OpenFlex`）/ `StudioSurface` |
| `flowy-app` | 域状态 + 壳装配；持有 `Entity<StudioPanel>`；登录 UI 只绑 auth ViewModel；Flex 模态 + `preferences.json` | `FlowyApp` / `app::run`；`pub use` agent/i18n |
| `flowy-desktop` | 二进制入口（bin 名 `flowy-app`） | `dx serve --hot-patch` / `--hotpatch` 宿主 |
| `dsh-rs` | Agent harness | 仅 `api` / `types` / `bundle`（`unstable` 给 CLI/测试） |

热补丁入口：

- Windows PowerShell：`powershell -ExecutionPolicy Bypass -File scripts/dev-hotpatch.ps1`
- macOS：`bash scripts/dev-hotpatch.sh`

Windows 推荐启动步骤：

1. 关闭已有的 Flowy 窗口，确保没有 `flowy-app.exe` 进程。
2. 在仓库根目录执行：

   `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev-hotpatch.ps1`

3. 保持这个终端和 `dx serve` 进程运行，修改源码后观察窗口热补丁结果。

首次启动会编译完整 debug 依赖，耗时较长；后续应在同一个 `dx serve` 会话中迭代。脚本会在编译前检查旧进程、`dx` 0.7.10、Visual Studio 的 `link.exe` 和 Windows SDK 的 `advapi32.lib`，并自动设置 MSVC/SDK 的 `LIB` 路径。两个入口都使用 GPUI 的 `hot-patching` feature。Windows 入口会把 `link.exe` 传给 Cargo，避免仓库默认的 `rust-lld.exe` 与 Dioxus hotpatch linker 冲突。热补丁只对 debug/native 构建生效，修改渲染、布局和绘制逻辑时会保留运行中的窗口状态；修改 struct 字段、初始化逻辑或依赖状态布局时需要重启。
