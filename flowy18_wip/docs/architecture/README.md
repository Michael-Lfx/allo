# 架构与决策

[← 文档总索引](../README.md)

| 文档 | 状态 | 用途 |
| --- | --- | --- |
| [router-work-and-tool-extension.md](router-work-and-tool-extension.md) | ✅ 底座已实现并硬化收口（`PR #530` @ `58f88471a` + `PR #534` @ `2b783e385`） | Router 与 Worker 角色/物理工具隔离全景、有状态 `ToolProvider` 接口（含 `version()` 绑定、锁外预装配与重名拒绝）、通用 `Ask` 防过期防绕过审批、`Deferred`/`tool_search` 快照与上限接入指南 |
| [flowy-v0/README.md](flowy-v0/README.md) | 修订 9.23（基线 `origin/main @ f40a16275`，含 `PR #530–#545`） | Flowy 1.8 本地优先 Agent 内核架构方案、工作契约、OOXML/L3/UI/ScrollArea 门禁、跨平台沙箱、个人自动化与实测证据索引 |
| [agent-core-migration.md](agent-core-migration.md) | 迁移已完成，收尾项见 plans | agent 内核从 dsh-rs 迁到自有 `flowy-agent-core` 的进度、验证与修复记录 |
| [agent-shell-strategy.md](agent-shell-strategy.md) | 已采纳 | 多端 shell 决策：Windows 使用 PowerShell，macOS/Linux 使用 bash |
