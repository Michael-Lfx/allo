# 验证限制与阻塞

## 当前状态（2026-11-17 收尾）

本分支已完成账号模型目录、会话路由修复、pi-rust 内核迁移的收尾，并 rebase 到最新 `origin/main`
（`0cccfec79`）。相关验证：`flowy-app` 21 passed、`flowy-studio` 5 passed、
`flowy-agent-core` 24 passed/1 ignored、`flowy-agent` 34 passed/2 ignored、`flowy-auth` 11 passed；
Windows 桌面入口实跑可启动（窗口出现、目录模型名同步到顶栏/输入框）。

## 已闭环

- 会话路由不随 `apply_config` 更新：迁移到 `flowy-agent-core`（pi-rust loop 内核）后，
  `Engine` 每轮从当前 `config` 读取路由，同一 session 切换模型保留历史；
  `config_switch_applies_to_the_next_run_on_the_same_session` 覆盖该回归。
- 选择保存成功后恢复 `cloud_ready` 并允许发送；目录空/刷新失败/未选择时保持发送关闭并给出提示。
- 损坏 `index.json` 的隔离与 history 重建已由测试覆盖（`failed_new_task_is_not_hidden_by_navigation`）。

## 当前限制

- 真实云端推理未实测（本功能只用固定 fixture 与路由配置做验证）。
- 其他平台（macOS / Linux）未构建、未实测。
- Windows MSVC D8050 根因未闭环：本分支用「VS 终端 + 关闭原生调试信息」
  （`CARGO_PROFILE_DEV_DEBUG=0` 或 `profile.dev.package."*".debug=false`）完成验证，
  根因留到下一个专门分支，见 [../plans/windows-build-rebase-pitfalls.md](../plans/windows-build-rebase-pitfalls.md)。
- 本机 auth/agent 的部分存储测试需要把 `TEMP`/`TMP` 指到工作区内可写目录
  （默认临时目录权限拒绝），属环境限制，不是产品缺陷。
- 目录 fixture 为固定 JSON 样本（首条来自已知样本，其余合成），不是完整云端真实目录。
- 历史工具结果消息缺少 `MessageSource::Tool` 时会让整轮失败
  （`flowy-agent-core/src/loop_runner.rs::to_agent_message`）；容错修复留到下个分支。
- 未逐条核对 23 个 locale 的翻译质量（新增 key 见 `flowy-i18n` 变更）。

## 历史记录

第 4—6 轮记录的「会话路由不更新」阻塞已由上述迁移修复，原文保留在 git 历史与本目录
`PROGRESS.md` 的早期条目中，仅供追溯。
