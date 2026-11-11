| Crate | 职责 | 对外 |
| ----------- | ------------------------- | ------------------ |
| `flowy-net` | 代理 + Client 工厂 | 传输钩子 |
| `flowy-ota` | 镜像、清单、rollout、验签、下载缓存槽、各平台安装 | `Updater` + `DownloadedUpdateCache` |
| `flowy-obs` | 日志、Sentry 崩溃上报、遥测 allowlist/outbox | `init_tracing` / `Telemetry` |
| `flowy-core` | 文档图模型、历史、原子写、`DocumentSession` | 会话命令 API |
| `dsh-rs` | Agent harness | 仅 `api` / `types` / `bundle`（`unstable` 给 CLI/测试） |
| `flowy-app` | 桌面壳 + 域状态 + shell 生命周期 | `UpdateStatus` / deep link / tray / notify / autostart |
