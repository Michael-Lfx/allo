---
id: g1
title: "生长链接用户模型选择——grow 路由+前端接线 modelChoice"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# g1: 生长链接用户模型选择

**What to build:** grow 路由收 `provider_id+model`（成对校验，复用
regenerate/mount/repair 同款模式 `routes.rs:502-506`），透传给
`kick_growth_with_trigger`（服务层 `model_override` 参数现成，
learning_graph.rs:1646）；前端 `api.ts` `growGraph` 带 modelChoice
（`useLearningAutogenModel`），`LearningGraphWorkspace.tsx:326` 接线；
auto_refill 自动触发端点若有同款硬编码 None 一并处理。

**验收：**
- 用户选择器选 A 后触发生长，nomi.log SSE 请求 model=A（不再回落平台默认档）
- 只给一个字段时 400（成对约束）
- `cargo test -p nomifun-learning` + `bun run typecheck` 绿

**病历依据：** 2026-11-09 真机 growth 948s 墙钟 73% 浪费在默认档
v4-flash 的 reasoning 方差+网关抖动；同选择课时链 v4.1-flash
14 连调 1.5–8.8s/次。

**施工补充（探察 2026-11-10）：** auto_refill 无独立端点——前端
`useAutoRefill` 复用同一个 `POST grow?trigger=auto_refill`。前端接线
时手动与自动两条路都带 modelChoice（自动触发读持久化选择）；
`kick_growth_with_trigger` 全部调用方清单见共享笔记 g1.md。

## Resolution

已关（2026-11-10）：commit `340f3bc4d`，集成分支经 `c43dae34e` 并回 feat/learn0928（合并 `49f02cd2f`）。手动+自动补批双路携带 modelChoice，路由级成对校验/透传单测在场；修复轮后全量 952 Rust / 133 UI / typecheck 基线持平。
