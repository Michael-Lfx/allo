# 架构与决策

[← 文档总索引](../README.md)

| 文档 | 状态 | 用途 |
| --- | --- | --- |
| [router-work-and-tool-extension.md](router-work-and-tool-extension.md) | ✅ 底座已实现（`feat/flowy18-work-capability-provider`） | Router 与 Worker 角色/物理工具隔离全景、有状态 `ToolProvider` 接口、通用 `Ask` 审批消费、`Deferred`/`tool_search` 按需激活与防冲突接入指南 |
| [flowy-v0/README.md](flowy-v0/README.md) | 修订 9.4（已实现并合入 `feat/flowy-v0-rc`） | Flowy 1.8 本地优先 Agent 内核架构方案、工作契约、OOXML/L3 核验门禁、跨平台沙箱预留蓝图与实测证据索引 |
| [agent-core-migration.md](agent-core-migration.md) | 迁移已完成，收尾项见 plans | agent 内核从 dsh-rs 迁到自有 `flowy-agent-core` 的进度、验证与修复记录 |
| [agent-shell-strategy.md](agent-shell-strategy.md) | 已采纳 | 多端 shell 决策：Windows 使用 PowerShell，macOS/Linux 使用 bash |
