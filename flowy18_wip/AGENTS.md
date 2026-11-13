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
| `flowy-desktop` | 二进制入口（bin 名 `flowy-app`） | `dx serve --hot-patch` 宿主 |
| `dsh-rs` | Agent harness | 仅 `api` / `types` / `bundle`（`unstable` 给 CLI/测试） |

热补丁：`scripts/dev-hotpatch.sh`（`dx` 0.7.10 + `gpui` feature `hot-patching`）。不丢窗口状态；改 struct 字段需重启。
