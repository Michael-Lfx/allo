# Agent 内核解耦 — 收尾交接

状态：六步迁移已落地，`flowy-agent` 依赖树只剩 `flowy-agent-core`、`serde`、`serde_json`、`tokio`；`dsh-rs` 已移除。以下为尚未完成的验收与行为补齐项，供决定是否启动。

## 1. 上层消费者编译验证

- 目标：确认 `flowy-studio`、`flowy-app`、`flowy-desktop` 在公开 API 不变的情况下编译通过。
- 命令：`cargo check -p flowy-studio -p flowy-app`（Windows 需 MSVC 包装；本机 registry 需覆盖 `source.crates-io.replace-with="rsproxy-sparse"`）。
- 验收：退出码 0。
- 现状：静态审计显示所用方法全部存在、`SessionSummary` 字段未变；整包编译因 GPUI/Dioxus 依赖在图构建耗时过长被中止，未取得退出码。缓存已预热，重跑应显著加快。

## 2. 恢复标题 sidecar

- 目标：恢复迁移前的第二路模型标题；失败时保留本地回退标题。
- 方案：在 core 增加一次无工具的 `run_turn`（title system prompt + 首轮用户文本），成功后写回 `title`；不阻塞主 turn。
- 验收：有 key 时标题来自模型且不超过 `TITLE_MAX_CHARS`；无 key 或调用失败时仍为 `fallback_title`；新增对应测试。

## 3. 推理增量投影

- 目标：模型思考增量进入 `ChatLine::Reasoning { live: true }`，turn 结束时置 `live=false` 并写 `duration_secs`。
- 方案：core 的 `TurnObserver` 增加 thinking delta 回调；handle 的投影追加/合并 reasoning 行。
- 验收：流式期间可见 reasoning 行，结束后状态正确；恢复历史时原样保留。

## 4. 真实云端 SSE 冒烟

- 目标：用真实账号 token 走一次 `send`，确认 pi-ai 传输与网关兼容。
- 验收：`turn_status()` 到 `Completed`，助手文本非空，无密钥进入日志；失败时记录状态码与错误体（脱敏）。
- 注意：本地网络不可用时不得阻塞合并，记录为环境限制。

## 5. 分发与镜像

- 目标：让默认 registry（本机 `flowy18_wip/.cargo/config.toml` 指向的镜像）包含 pi 依赖，或改为可达源/ vendor。
- 验收：`cargo check -p flowy-agent --offline` 在干净缓存下成功；CI 不依赖 GitHub 直连。
- 现状：缺 `eventsource-stream`，只能通过 CLI 覆盖源完成构建。

## 证据索引

- core：`cargo test -p flowy-agent-core` → 26 passed。
- agent：`cargo test -p flowy-agent`（TEMP 重定向）→ 17 passed / 1 ignored。
- 依赖树：`cargo tree -p flowy-agent --depth 1` → core / serde / serde_json / tokio。
- 进度记录：[agent-core-migration.md](../architecture/agent-core-migration.md)。
