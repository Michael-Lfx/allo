---
id: m1
title: findings 可见——读接口 + 结构页签展示（零 LLM）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

评审把看见的缺陷写进了 `learning_structure_findings`（本课已积 16 条），但
**没有任何读接口、没有任何界面**。用户看不到「评审看见了什么」，也看不到
「哪些还没修」。违反用户铁律：上下文里有 ≠ 用户能看到。

## 已核验事实

- 表 `learning_structure_findings`：`seq / source / node / field / issue /
  advice / course_id`（`070_learning_domain_rebuild.sql`）。现有 source 值：
  `concept-review`、`growth-budget`。
- 写侧：`append_structure_findings`（service，fail-open）；读侧只有两处内部
  用途——修补回合读水位之后的行、`maintenance_needs` 数未读行数。**无对外
  接口**。
- 水量语义：`applied` 与 `noop` 都推进（= 已读，不是已修复）。
- UI 现状：结构图页签已有「详情」折叠区（挂载观察逐行展示），是并列放置
  留痕的现成位置（`StructureGraphTab.tsx` 挂载明细折叠）。

## R

- **R-a 读接口**：`GET /api/learning/courses/{id}/structure/findings?limit=N`
  （默认 50，上限 200）——按 `seq DESC` 返回最近的留痕，字段照表返回，另加
  `read_by_repair: bool`（`seq <= 水位` = 修补已读过）。纯读、零 LLM、零写。
  课程不存在/软删 → 404（与 structure_view 同口径）。
- **R-b UI**：结构图页签的「详情」折叠区新增一段「评审留痕（最近 N 条）」：
  每条显示 `field`｜`issue`（原文）｜`advice`（原文，可为空）｜来源标记
  （concept-review / growth-budget）｜「修补已读」小标。默认折叠，不打扰。
  空态 = 整段不渲染。
- **R-c 双语**：新增文案进 zh-CN / en-US 两份词典并对称；`bun run gen:i18n`
  更新 `i18n-keys.d.ts`；`bun run check:i18n` 通过。
- **R-d 不做**：不提供编辑/删除/标记处置（留痕是只增审计面）；不推进水位；
  不触发任何回合。

## Acceptance

- 接口单测：按 seq 倒序、limit 生效、`read_by_repair` 与水位一致（构造
  `learning_maintenance_state` 行验证两侧）、课程不存在 404。
- UI 测试：有条目时渲染、空态不渲染、`read_by_repair` 标记正确。
- `cargo test -p nomifun-learning` 全量绿 + `bun run typecheck` +
  `bun run check:i18n`。
- **真机**：起服务后该课结构页签「详情」里能看到那 16 条（含「Option 类型」
  「容器/接口」「前置底座缺口」三条原文）。

## 回退条件

纯读面，无回退风险。若 UI 位置干扰结构图操作 → 挪进二级折叠即可。
