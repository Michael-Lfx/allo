# pi-rust 迁移后续收尾（feat/flowy18-wip-followups）

## 背景

PR #458（`experiment/pi-loop-contracts`）合并后，`flowy18_wip` 的 Windows CI job 第一次真正运行，
门禁债在合并前已清零。本分支处理审查阶段遗留、当时约定「下个分支」完成的项。

## 范围与验收

1. 历史工具结果容错：`flowy-agent-core::to_agent_message` 遇到缺少 `MessageSource::Tool` 的
   工具结果消息时返回 `Ok(None)` 跳过，不再让整轮失败；回归测试
   `legacy_tool_result_without_tool_source_is_skipped`。
2. 失败状态可见：`flowy-studio` 的 `WorkingIndicator` 在 `TurnStatus::Error` 时显示本地化错误行
   （`circle_alert` + danger 色，`aria_live=Polite`），key/raw 继续由 `status_error_label` 脱敏；
   单测 `failed_turn_keeps_the_working_line_visible`。
3. 刷新门禁：`catalog_refresh_allowed(loading, has_session, busy)` 纯函数 + 单测
   `catalog_refresh_gate_is_single_flight_for_idle_sessions`。
4. D8050：默认 profile 复测结论与 workaround 记录见
   [windows-build-rebase-pitfalls.md](windows-build-rebase-pitfalls.md)。
5. 文档：`docs/cloud-model-catalog/BLOCKED.md`、`PROGRESS.md` 同步。

## 验证

- `cd flowy18_wip && cargo fmt --all -- --check`（Rust 1.100）
- `cargo clippy --offline --config 'profile.dev.package."*".debug=false' -p flowy-ui -p flowy-studio -p flowy-app -p flowy-i18n --all-targets --locked -- -D warnings`
- `cargo test -p flowy-agent-core -p flowy-agent -p flowy-app -p flowy-studio -p flowy-auth -p flowy-core -p flowy-net -p flowy-ota -p flowy-obs -- --test-threads=1`
  （`TEMP`/`TMP` 需分两条 export 到工作区内目录）

## 未覆盖

- 真实云端推理、macOS/Linux 构建、D8050 根因（当前环境不可复现）。
