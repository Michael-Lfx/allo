UI / 弹窗 / 圆角裁切规则见 [`DESIGN.md`](DESIGN.md)。做 GPUI 界面时必须遵守。

**GPUI 本身用 `gpui-pre` + `gpui-kit`，不是社区 fork `gpui-ce`。** 写任何 GPUI 代码前先读 [`docs/gpui-pre-and-kit.md`](docs/gpui-pre-and-kit.md)——它记录了三个依赖为什么必须精确锁定、`gpui_kit::base::input` 的路径、必须传 `window` 的构造、程序化写值的两个语义陷阱、`gpui-pre` 缺失的 11 项能力及替代方案、键上下文按深度取最深匹配的规则（这条踩过坑）、六条布局陷阱和测试的三个坑。

Windows 上 rebase / 依赖 / MSVC 编译 / 跑测试的环境问题，先看 [`docs/plans/windows-build-rebase-pitfalls.md`](docs/plans/windows-build-rebase-pitfalls.md)；文档总索引在 [`docs/README.md`](docs/README.md)。

已知能力缺口与取舍记录在 [`BLOCKED.md`](BLOCKED.md)，迁移进度在 [`PROGRESS.md`](PROGRESS.md)。动手改界面行为前扫一眼 `BLOCKED.md`，那里列的十条是**有意不做**，不是漏做。

| Crate | 职责 | 对外 |
| ----------- | ------------------------- | ------------------ |
| `flowy-net` | 代理 + Client 工厂 | 传输钩子 |
| `flowy-ota` | 镜像、清单、rollout、验签、下载缓存槽、各平台安装 | `Updater` + `DownloadedUpdateCache` |
| `flowy-obs` | 日志、Sentry 崩溃上报、遥测 allowlist/outbox | `init_tracing` / `Telemetry` |
| `flowy-core` | 文档图模型、历史、原子写、`DocumentSession` | 会话命令 API |
| `flowy-auth` | 云登录 Email OTP + JWT 会话落盘；经 `flowy-net` 调 Flowy `/user/*` | `AuthClient` / `SessionStore` / `Session`；env：`FLOWY_API_BASE_URL` / `FLOWY_AUTH_CHANNEL` / `FLOWY_AUTH_APP` |
| `flowy-chat` | 任务线程投影 + mock stream | `Thread` / `Item` / `Beat` / `MockStream` |
| `flowy-ui` | 窗口主题、控件与弹层契约、Accent/Density | `UiContext` / `ThemeState` / `ResolvedTheme` / `ModalLayer` / 控件 builders / `OverlayHost` |
| `flowy-shell` | deep link / notify / autostart / 托盘 | `install(Brand, open_main)` / `configure_brand` / `tray_available`；托盘走 `gpui-tray` 的 `gpui-kit` feature（真包是 `gpui-pre`）。托盘可用时关窗最小化（`gpui-pre` 没有 `set_visible`），不可用时关窗退出；`ctrl-q` 始终可退 |
| `flowy-agent` | UI↔core 端口；多 session / 流式投影 / 标题回退；`Model`/`Plan`/`FlexProvider` | `AgentHandle` / `ChatLine` / `TurnStatus`；env：`FLOWY_LLM_API_KEY` / `FLOWY_LLM_BASE_URL` / `FLOWY_LLM_MODEL`；Flex BYOK 入口当前未接线（`LlmConfig::from_flex` 保留） |
| `flowy-agent-core` | 自有 agent 内核：类型、工具注册、结构化历史、pi loop、OpenAI 兼容传输 | `Engine` / `ToolRegistry` / `run_turn` / `TurnObserver` |
| `flowy-i18n` | 23 locales + `{product}` | `I18n` / `Locale` |
| `flowy-studio` | Home / Chat / Automation Entity | `StudioPanel` / `StudioEvent`（含 `OpenFlex`）/ `StudioSurface` |
| `flowy-app` | 域状态 + 壳装配；持有 `Entity<StudioPanel>`；登录 UI 只绑 auth ViewModel；Flex 模态 + `preferences.json` | `FlowyApp` / `app::run`；`pub use` agent/i18n |
| `flowy-desktop` | 二进制入口（bin 名 `flowy-app`） | `dx serve --hot-patch` / `--hotpatch` 宿主 |

热补丁入口：

> **在 `gpui-pre` 下本机不可用。** `gpui-pre` 的 `hot-reload` feature 是真的（基于 Dioxus subsecond，每个 view 的 `render` 都在 `HotFn` 边界里），但 `dx serve --hot-patch` 会强制 `-Clinker=dx.exe`，在链接阶段失败（`exit 1` / `LNK1104`）。用 cargo 走同一个 wrapper 能构建成功，所以问题在 dx 的调用参数，不在 MSVC 或 wrapper。下面这套流程沿用自 `gpui-ce` 时代，保留备查，但**现在跑不通**。

- Windows PowerShell：`powershell -ExecutionPolicy Bypass -File scripts/dev-hotpatch.ps1`
- macOS：`bash scripts/dev-hotpatch.sh`

Windows 推荐启动步骤：

1. 关闭已有的 Flowy 窗口，确保没有 `flowy-app.exe` 进程。
2. 在仓库根目录执行：

   `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev-hotpatch.ps1`

3. 保持这个终端和 `dx serve` 进程运行，修改源码后观察窗口热补丁结果。

首次启动会编译完整 debug 依赖，耗时较长；后续应在同一个 `dx serve` 会话中迭代。脚本会在编译前检查旧进程、`dx` 0.7.10、Visual Studio 的 `link.exe` 和 Windows SDK 的 `advapi32.lib`，并自动设置 MSVC/SDK 的 `LIB` 路径。两个入口都使用 GPUI 的 `hot-patching` feature。Windows 入口会把 `link.exe` 传给 Cargo，避免仓库默认的 `rust-lld.exe` 与 Dioxus hotpatch linker 冲突。热补丁只对 debug/native 构建生效，修改渲染、布局和绘制逻辑时会保留运行中的窗口状态；修改 struct 字段、初始化逻辑或依赖状态布局时需要重启。
