# 计划与交接

[← 文档总索引](../README.md)

| 文档 | 状态 | 用途 |
| --- | --- | --- |
| [local-first-ui-v2.zh.md](local-first-ui-v2.zh.md) | 已实现并收尾（2026-11-25，分支 `feat/flowy-local-ui-v2` @ `a0fe5df13`，基线 `feat/flowy-v0-rc` @ `10b99bac0`） | Local-First UI v2 可视化工作台看板、居中流程图弹窗、4 筹码驾驶舱、`flowy-agent` 6+4 接口全闭环、模块拆解、外部打开安全门禁、ADR 权衡与踩坑溯源手册 |
| [super-agent-ui-and-motion.zh.md](super-agent-ui-and-motion.zh.md) | 计划（2026-11-25 架构与交互定稿） | Super Agent 常驻助理/项目空间侧栏、全程实时折叠时间线、悬浮任务胶囊、GPUI 120fps 丝滑动效映射与双分支并行方案 |
| [personal-agent-automation-execution.md](personal-agent-automation-execution.md) | 实施中（2026-11-26，4/31） | 分批实施、每步验证；PA-A01、PA-A02、PA-A04、PA-B01 已完成并验证；下一项 PA-A03 |
| [personal-agent-automation-development-plan.md](personal-agent-automation-development-plan.md) | 实施中（2026-11-26，4/31 已完成） | Personal Agent 对话式自动化：31 项可执行任务、五阶段依赖、六项审查修订、24 项验收映射与 V2 完整快照 |
| [clawhub-skill-market.md](clawhub-skill-market.md) | 已实现（2026-11-23） | ClawHub 社区货架、官方 JSON、SQLite 缓存、审核过滤与安装门禁 |
| [conversation-artifact-library-execution.zh.md](conversation-artifact-library-execution.zh.md) | 实施与验证中 | 代码链路、自动化证据、TC-01–35 与待完成原生/发行门禁 |
| [conversation-artifact-library.zh.md](conversation-artifact-library.zh.md) | 计划（2026-11-21，产品范围已确认） | 首页对话文件交付、自动归集、独立存储、共享预览与来源定位的完整实施计划 |
| [agent-core-migration-handoff.md](agent-core-migration-handoff.md) | 待启动 | agent 内核迁移后的收尾项、验收集与行为补齐项 |
| [agent-observability-plan.md](agent-observability-plan.md) | 待确认 | 为每次 agent turn 增加本地可查询轨迹的最小方案 |
| [cloud-provider-thin-plan.md](cloud-provider-thin-plan.md) | 可行性已验证，待实现 | 云端单一 provider 接入方案 |
| [dsh-rs-removal-plan.md](dsh-rs-removal-plan.md) | 已完成（2026-11-17） | 删除 dsh-rs、只保留 pi-rust 的计划与回滚说明 |
| [local-agent-evidence.zh.md](local-agent-evidence.zh.md) | 已核对源码（2026-11-19） | 记录开源借鉴依据与 Flowy 采用范围 |
| [local-agent-design.zh.md](local-agent-design.zh.md) | 方案待实现 | 统一入口、Router、pi-rust 任务运行和 GPUI 渲染，附历次讨论 |
| [local-agent-execution.zh.md](local-agent-execution.zh.md) | 待授权实施 | 点子王审查、执行阶段、基线与首个可复制任务书 |
| [document-preview-dependencies.zh.md](document-preview-dependencies.zh.md) | 调研结论（2026-11-20） | GPUI CE / GPUI Web 中 PPTX、PDF、JPG 预览的依赖选型与兼容性 |
| [pi-loop-followups.md](pi-loop-followups.md) | 已收尾（2026-11-18，待合入） | PR #458 合并后的遗留项：工具结果容错、失败状态可见、刷新门禁、D8050 复测 |
| [rust-1.100-alignment.md](rust-1.100-alignment.md) | 已对齐（2026-11-18） | 工具链统一到 Rust 1.100：MSRV、CI pin、Cargo 1.100 未使用依赖清理与验证记录 |
| [session-index-storage-error-test.md](session-index-storage-error-test.md) | 已修复（2026-11-17） | 索引损坏场景下 storage_error 测试的失败原因、修复与验证记录 |
| [windows-build-rebase-pitfalls.md](windows-build-rebase-pitfalls.md) | 已验证（Windows，2026-11-26 增补热补丁） | Windows 上 rebase / 依赖 / MSVC 编译 / 跑测试 / `dx serve --hot-patch` 热补丁的踩坑与照抄命令 |
