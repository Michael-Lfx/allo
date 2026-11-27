UI / 弹窗 / 圆角裁切规则见 [`DESIGN.md`](DESIGN.md)。做 GPUI 界面时必须遵守。

**GPUI 本身用 `gpui-pre` + `gpui-kit`，不是社区 fork `gpui-ce`。** 写任何 GPUI 代码前先读 [`docs/gpui-pre-and-kit.md`](docs/gpui-pre-and-kit.md)——它记录了三个依赖为什么必须精确锁定、`gpui_kit::base::input` 的路径、必须传 `window` 的构造、程序化写值的两个语义陷阱、`gpui-pre` 缺失的 11 项能力及替代方案、键上下文按深度取最深匹配的规则（这条踩过坑）、六条布局陷阱和测试的三个坑。

Windows 上 rebase / 依赖 / MSVC 编译 / 跑测试 / `dx serve` 热补丁的环境问题，先看 [`docs/plans/windows-build-rebase-pitfalls.md`](docs/plans/windows-build-rebase-pitfalls.md)；文档总索引在 [`docs/README.md`](docs/README.md)。

已知能力缺口与取舍记录在 [`BLOCKED.md`](BLOCKED.md)，迁移进度在 [`PROGRESS.md`](PROGRESS.md)。动手改界面行为前扫一眼 `BLOCKED.md`，那里列的十条是**有意不做**，不是漏做。

| Crate | 职责 | 对外 |
| ----------- | ------------------------- | ------------------ |
| `flowy-net` | 代理 + Client 工厂 | 传输钩子 |
| `flowy-ota` | 镜像、清单、rollout、验签、下载缓存槽、各平台安装 | `Updater` + `DownloadedUpdateCache` |
| `flowy-obs` | 日志、Sentry 崩溃上报、遥测 allowlist/outbox | `init_tracing` / `Telemetry` |
| `flowy-core` | 文档图模型、历史、原子写、`DocumentSession` | 会话命令 API |
| `flowy-auth` | 云登录 Email OTP + JWT 会话落盘；经 `flowy-net` 调 Flowy `/user/*` | `AuthClient` / `SessionStore` / `Session`；env：`FLOWY_API_BASE_URL` / `FLOWY_AUTH_CHANNEL` / `FLOWY_AUTH_APP` |
| `flowy-skills` | 本地与远程技能注册表、安全边界、预算裁剪与提示注入 | `SkillRegistry` / `SkillSummary` / 技能市场同步 |
| `flowy-ui` | 窗口主题、控件与弹层契约、Accent/Density | `UiContext` / `ThemeState` / `ResolvedTheme` / `ModalLayer` / 控件 builders / `OverlayHost` |
| `flowy-shell` | deep link / notify / autostart / 托盘 | `install(Brand, open_main)` / `configure_brand` / `tray_available`；托盘走 `gpui-tray` 的 `gpui-kit` feature（真包是 `gpui-pre`）。托盘可用时关窗最小化（`gpui-pre` 没有 `set_visible`），不可用时关窗退出；`ctrl-q` 始终可退。注意：从带标签的沙箱目录直接运行 exe，进程是低完整性，Windows 会拒绝托盘图标，表现为关窗直接退出——装好或换目录运行即正常 |
| `flowy-agent` | UI↔core 端口；多 session / 流式投影 / 标题回退；`Model`/`Plan`/`FlexProvider` | `AgentHandle` / `ChatLine` / `TurnStatus`；env：`FLOWY_LLM_API_KEY` / `FLOWY_LLM_BASE_URL` / `FLOWY_LLM_MODEL`；Flex BYOK 入口当前未接线（`LlmConfig::from_flex` 保留） |
| `flowy-agent-core` | 自有 agent 内核：类型、工具注册、结构化历史、pi loop、OpenAI 兼容传输 | `Engine` / `ToolRegistry` / `run_turn` / `TurnObserver` |
| `flowy-i18n` | 23 locales + `{product}` | `I18n` / `Locale` |
| `flowy-studio` | Home / Chat / Board / Automation Entity | `StudioPanel` / `StudioEvent`（含 `OpenFlex`）/ `StudioSurface` |
| `flowy-app` | 域状态 + 壳装配；持有 `Entity<StudioPanel>`；登录 UI 只绑 auth ViewModel；Flex 模态 + `preferences.json` | `FlowyApp` / `app::run`；`pub use` agent/i18n |
| `flowy-desktop` | 二进制入口（bin 名 `flowy-app`） | `dx serve --hot-patch` / `--hotpatch` 宿主 |

热补丁入口（`gpui-pre` 的 `hot-reload` feature + Dioxus `0.7.10` subsecond，Windows 已验证跑通）：

- Windows PowerShell：`powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev-hotpatch.ps1`
- macOS：`bash scripts/dev-hotpatch.sh`

Windows 推荐启动步骤：

1. 关闭已有的 Flowy 窗口，确保没有 `flowy-app.exe` 进程。
2. 在 `flowy18_wip` 根目录（普通 PowerShell 即可，无需手动进 VS 开发者终端）执行：

   `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\dev-hotpatch.ps1`

3. 保持这个终端和 `dx serve` 进程运行，修改 UI 源码（`flowy-desktop` / `flowy-app` / `flowy-studio` / `flowy-ui` / `flowy-i18n` / `flowy-shell`）后观察窗口原位热补丁结果。

首次启动会编译完整 debug 依赖并打包 fat archive（`libdeps-*.a`），耗时较长；后续应在同一个 `dx serve` 会话中迭代。Windows 脚本会在编译前检查旧进程、`dx` 0.7.10、Visual Studio MSVC（自动经 `vswhere.exe` 查找并置于 `PATH` 前端以避开 MSYS `link.exe`）与 Windows SDK 的 `advapi32.lib`，自动设置 `LIB`，并通过 `scripts/dx-capture-wrapper`（`dx-capture-wrapper` + `dx-link-wrapper`）与 rustup 内置 `lld-link.exe`（缺失时回退 MSVC `link.exe /INCREMENTAL:NO`）规避 `dx 0.7.10` 在 Windows 下的响应文件引号、`NoLink` 与 `__chkstk` 桩缺陷（详见 [`docs/gpui-pre-and-kit.md`](docs/gpui-pre-and-kit.md) 与 [`docs/plans/windows-build-rebase-pitfalls.md`](docs/plans/windows-build-rebase-pitfalls.md)）。热补丁只对 debug/native 构建生效，修改渲染、布局和绘制逻辑时会保留运行中的窗口状态与进程 PID；修改 struct 字段、静态初始化逻辑或状态布局时按 `r` 重建或重启。

### Windows 同事与 Agent 速查（前置 · 快速验证 · 红线）

- **前置安装（一次性）**：Rust `1.100`（`rust-toolchain.toml` 已锁，自带 `lld-link.exe`）、Visual Studio Build Tools（MSVC x64 + Windows 10/11 SDK），以及精确版本 `cargo install dioxus-cli --version 0.7.10 --locked`。
- **分层验证（避免每次改几行都跑全量 8-crate 门禁）**：
  - 界面/布局/文案迭代：启动一次 `.\scripts\dev-hotpatch.ps1`，保存源码约 15–18s 原位热补丁生效。
  - 编译与逻辑内循环：`cargo check -p <crate> --lib`、`cargo test -p <crate> --lib <关键词> -- --nocapture`、跨 crate 签名核验用 `cargo check -p flowy-desktop`（跳过链接）、改 UI/i18n 跑 `bun scripts/check-ui-contracts.mjs`；`flowy-agent` 日常整 crate 复验可先跑 `-- --test-threads=4`（~19s）。
  - 仅 PR 提交前跑一次完整门禁：`bun scripts/check-ui-contracts.mjs --self-test && bun scripts/check-ui-contracts.mjs`、`cargo fmt --all -- --check`、受影响及 CI 必过 crate 单测（`flowy-ui` **禁带 `--lib`** 以免漏跑集成测试 `tests/anchor_measurement.rs`；`flowy-agent` 固定 `-- --test-threads=1` 规避 NTFS `fsync` 并发抖动）、`cargo check -p flowy-desktop --locked`、`cargo clippy ... --all-targets -- -D warnings`（改动 `flowy-agent-core`/`flowy-agent` 公开结构体时须同时覆盖下游 `flowy-studio`/`flowy-app`/`flowy-desktop`，防 `clippy::large_enum_variant` 与单测 `pedantic` 告警）。
- **Windows Agent 避坑红线**：
  - 必须经 `.\scripts\dev-hotpatch.ps1` 启动热补丁，**不要**手敲裸 `dx serve --hot-patch` 或手动注入 `RUSTC_WRAPPER=sccache` / `-Clinker=...`。
  - 同一 worktree 内**严禁并发运行多个 `cargo`**（会争抢 `build.noindex` 锁）；避免在同一次调试中交替跑 `-p flowy-studio -p flowy-agent` 与单独 `-p flowy-agent`（Cargo 特征统一哈希不同，首次切换会触发一次额外依赖重编）。
  - 不要删除 `scripts/dx-capture-wrapper/target/`（重编 `dx-capture-wrapper.exe` 会改变 Cargo 的 `RUSTC_WRAPPER` 指纹并触发 `desktop-dev` 依赖重编）。
  - 不要删掉 `crates/flowy-desktop/Cargo.toml` 中对 `flowy-i18n` / `flowy-studio` / `flowy-ui` 的直接依赖——它们用于让 `dx 0.7.10` 的 1-hop 监听器覆盖全部 UI crate。
