# ADR-0015: 枚举集校验移出 DB CHECK——读宽容、写严格

Status: Accepted

Date: 2026-11-02

## Context

学习域 15+ 处取值集列（题型 kind、节类型/状态、复习来源 source、XP 结算 kind/basis、
概念 role/tier…）以 SQLite `CHECK IN(...)` 钉死。SQLite 修改 CHECK 必整表重建：题型列已被
重建 3–4 次、状态列 2 次，每次扩一个值都是一次用户数据搬迁；上线后数据只增不减。而 Rust
enum 本就是这些取值集的单源，DB 约束是第二份手抄。

## Decision

枚举集列不再设 DB CHECK（数值域 0..1/0..4/1..10、布尔、json_valid、非空门保留），校验收口
到 Rust 单源：

- **写路径严格**：`TryFrom<&str>` 拒绝未知值，`as_str()` 是唯一写入口。
- **读路径宽容**：落库 enum 带 `Unknown(原文)` 兜底变体，未知值原样透传，前端可见降级
  （中性展示 + 原始码），绝不整行反序列化失败、绝不误标成相邻值。
- **互钉脚本**：把 Rust enum 变体、TS union、i18n key 与残留 CHECK 清单对拼，不一致即红
  （进 `bun run check`）。

与 v3 契约「无物理 FK/触发器，靠 `id_schema_contract.rs` 运行时登记」同一哲学：完整性
约束住在代码与契约登记，不住在 SQLite 方言里。已发布表的 CHECK 在学习域重建基线中一次性
移除。

## Considered Options

- 保留 CHECK、每次扩值重建：已发生 3–4 次，代价随数据增长。
- CHECK 预留未来值：猜取值集，猜错仍要重建，还污染读侧。

## Consequences

失去 DB 层对脏写的最后防线，由互钉脚本与写路径校验补回；排查脏数据时少一层 DB 报错、
多一层应用日志。
