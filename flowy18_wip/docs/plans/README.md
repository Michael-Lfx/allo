# 计划与交接

[← 文档总索引](../README.md)

| 文档 | 状态 | 用途 |
| --- | --- | --- |
| [agent-core-migration-handoff.md](agent-core-migration-handoff.md) | 待启动 | agent 内核迁移后的收尾项、验收集与行为补齐项 |
| [agent-observability-plan.md](agent-observability-plan.md) | 待确认 | 为每次 agent turn 增加本地可查询轨迹的最小方案 |
| [cloud-provider-thin-plan.md](cloud-provider-thin-plan.md) | 可行性已验证，待实现 | 云端单一 provider 接入方案 |
| [dsh-rs-removal-plan.md](dsh-rs-removal-plan.md) | 已完成（2026-11-17） | 删除 dsh-rs、只保留 pi-rust 的计划与回滚说明 |
| [pi-loop-followups.md](pi-loop-followups.md) | 已收尾（2026-11-18，待合入） | PR #458 合并后的遗留项：工具结果容错、失败状态可见、刷新门禁、D8050 复测 |
| [session-index-storage-error-test.md](session-index-storage-error-test.md) | 已修复（2026-11-17） | 索引损坏场景下 storage_error 测试的失败原因、修复与验证记录 |
| [windows-build-rebase-pitfalls.md](windows-build-rebase-pitfalls.md) | 已验证（Windows） | Windows 上 rebase / 依赖 / MSVC 编译 / 跑测试的踩坑与照抄命令 |
