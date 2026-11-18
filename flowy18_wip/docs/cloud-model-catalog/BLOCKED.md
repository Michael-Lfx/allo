# 验证限制与阻塞

## 当前状态（2026-11-18 收尾）

主分支已合并 PR #458（merge commit `86fcd6c32`），包含账号模型目录、会话路由修复、
pi-rust 内核迁移与 CI 门禁修复（格式 + 1.100 clippy）。后续分支 `feat/flowy18-wip-followups`
继续收尾遗留项。最新验证（Windows，Rust 1.100）：`flowy-app` 22 passed、
`flowy-studio` 9 passed、`flowy-agent-core` 39 passed/2 ignored、
`flowy-agent` 24 passed/1 ignored、`flowy-auth` 11 passed、`flowy-core` 33 passed、
`flowy-net` 3 passed、`flowy-ota` 23 passed、`flowy-obs` 6 passed；
Windows 桌面入口实跑可启动（窗口出现、目录模型名同步到顶栏/输入框）。

## 已闭环

- 会话路由不随 `apply_config` 更新：迁移到 `flowy-agent-core`（pi-rust loop 内核）后，
  `Engine` 每轮从当前 `config` 读取路由，同一 session 切换模型保留历史；
  `config_switch_applies_to_the_next_run_on_the_same_session` 覆盖该回归。
- 选择保存成功后恢复 `cloud_ready` 并允许发送；目录空/刷新失败/未选择时保持发送关闭并给出提示。
- 损坏 `index.json` 的隔离与 history 重建已由测试覆盖（`failed_new_task_is_not_hidden_by_navigation`）。
- 历史工具结果缺失时不再拖垮整轮：`to_agent_message` 跳过缺 `MessageSource::Tool` 的条目，
  `repair_tool_call_history` 在请求前补齐配对——缺结果的 tool call 合成 `is_error=true` 占位
  （与 pi harness 的 `repair_orphaned_tool_calls` 同策略）、无 call 的 tool result 丢弃、
  坏 JSON arguments 降级为空对象；回归测试
  `orphan_tool_call_gets_placeholder_and_orphan_results_are_dropped`、
  `unmatched_tool_result_is_dropped_from_provider_history`、
  `invalid_tool_arguments_degrade_to_an_empty_object`、
  `legacy_tool_result_without_tool_source_is_skipped`。
- 失败状态在工作行（feed 与 composer 之间）可见：`WorkingIndicator` 对 `TurnStatus::Error` 显示本地化错误行
  （`circle_alert` + danger 色，`aria_live=Polite`），key/raw 文本继续由 `status_error_label` 脱敏；
  单测 `failed_turn_keeps_the_working_line_visible`。
- 模型刷新门禁抽成纯函数 `catalog_refresh_allowed`（单飞、需登录、非 busy），
  单测 `catalog_refresh_gate_is_single_flight_for_idle_sessions`。
- CI 门禁（`flowy18_wip` 的 Windows job）已修复：`cargo fmt --check` 与
  `cargo clippy -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n --all-targets --locked -- -D warnings`
  在 1.100 工具链下均通过。注意 `flowy-agent-core` / `flowy-agent` 不在该 job 的 clippy 与测试步骤内，
  它们的门禁清理与回归测试是本地按同口径验证的；把两者纳入 CI 需要改 workflow，留待后续单独决定。

## 当前限制

- 真实云端推理未实测（本功能只用固定 fixture 与路由配置做验证）。
- 其他平台（macOS / Linux）未构建、未实测。
- Windows MSVC D8050 根因仍未定论，但 2026-11-18 在默认 profile（不关调试信息）、
  VS 环境、独立 target 目录下复测 `cargo check -p flowy-net`（33.52s，含 `ring`）与
  `cargo check -p flowy-app --locked`（7m29s，含 `cc`/`ring`/`gpui`）**均未复现**；
  workaround（`profile.dev.package."*".debug=false`）保留备用，不固化默认配置，见
  [../plans/windows-build-rebase-pitfalls.md](../plans/windows-build-rebase-pitfalls.md)。
- 本机 auth/agent 的部分存储测试需要把 `TEMP`/`TMP` 指到工作区内可写目录
  （默认临时目录权限拒绝），属环境限制，不是产品缺陷。
- 目录 fixture 为固定 JSON 样本（首条来自已知样本，其余合成），不是完整云端真实目录。
- 未逐条核对 23 个 locale 的翻译质量；本分支未新增 key，错误行复用既有 `chat.status_error`。

## 历史记录

第 4—6 轮记录的「会话路由不更新」阻塞已由上述迁移修复，原文保留在 git 历史与本目录
`PROGRESS.md` 的早期条目中，仅供追溯。
