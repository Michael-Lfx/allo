---
id: t4
title: "持久化：单个 course_source_plan_json 快照列 + 迁移（owner 签核）"
type: task
status: open
assignee:
blocked_by: [t1, t2, t3]
part_of: ../map.md
---

# t4: 持久化与迁移

**What to build:** 用**一个版本化 JSON 列** `learning_courses.course_source_plan_json` 承载建课时点的来源计划快照：

```json
{
  "version": 1,
  "inventory": { "...": "..." },
  "triage": { "...": "..." },
  "plan": { "...": "..." },
  "coverage": { "...": "..." }
}
```

旧课程/描述流/未启用新管线的课程写 NULL。**数据库迁移是高危区**：实施前必须 owner 签核；迁移只追加、不改写既有 068；编号以实施时 `migrations/` 最大号 +1 为准（当前最大 068，预计 069）。

**Blocked by:** [t1](t1-inventory-and-triage.md)、[t2](t2-evidence-driven-structure.md)、[t3](t3-batched-expansion.md)（字段形状以三票为准）。

**Status:** principle-approved / final-signoff-pending（t1–t3 形状冻结后）

## Grill 第一轮/第二轮定案（2026-10-21）

- **单列**：一个 `course_source_plan_json`，不再拆四个列；原子快照、一次迁移、字段演进容易。
- **顺序**：t1–t3 先用内存/WS 报告；字段形状稳定后 t4 一次性迁移，避免四次迁移返工。
- **签核（Q14）**：现在原则同意单列；t1–t3 字段形状冻结后由 owner 做最终迁移签核，之后才进入实现。
- **兼容**：NULL = 旧课程/描述流/未启用；读侧不得因 NULL 报错或伪造空 plan。

## 实施要点

- [ ] **迁移**：`migrations/069_learning_course_source_plan.sql`（编号以实施时最大号 +1 为准）：
  - `ALTER TABLE learning_courses ADD COLUMN course_source_plan_json TEXT;`
  - `CHECK (course_source_plan_json IS NULL OR (json_valid(course_source_plan_json) AND json_type(course_source_plan_json) = 'object'))`
  - 不写默认值、不重写旧行；`samples_meta_json`/`blueprint_json` 语义不变。
- [ ] **写入**：扩展 `OutlineSnapshot`/`import_course_outline`，在课程导入同一事务写新列；描述流/legacy/无 plan 写 NULL。
- [ ] **序列化**：`CourseSourcePlanSnapshot { version, inventory, triage, plan, coverage }`；版本号用于未来字段演进；未知版本读侧降级为 `unavailable`，不崩溃。
- [ ] **体积**：`inventory` 只存 path/size/hash/headings/preview ≤200 字，不存全文；`plan`/`coverage` 存结构化骨架与 source anchors，不存 lesson 正文；单库 1000 文件也需可控。
- [ ] **读侧**：`CourseSummary`/`CourseDetail` 增加可选 `source_plan` 视图（t5 定 DTO），只读快照；不 join KB、不实时重算。
- [ ] **迁移测试**：旧行 NULL 可读；新行必须 JSON object；非法 JSON 被 CHECK 拒绝；`cargo test -p nomifun-db` 全绿。
- [ ] **v3 契约**：加列不改表计数；确认 `id_schema_contract`/`PRODUCT_TABLES`/`LOGICAL_REFERENCES` 是否需要登记（新表才需要）；schema 测试全绿。
- [ ] **事务/取消**：导入中途失败/取消 = 零残留；不得出现“课程行有 plan、modules 没有”的半成品。
- [ ] **回滚方案**：迁移只加列；回滚 = 读侧忽略新列（不改旧列）；不删除数据。

## 非目标

- 不做 UI/WS（t5）。
- 不做历史版本表/多版本 plan 对比（v1 只存当前快照；后续需要再立票）。
- 不改 `learning_goal`/`samples_meta_json`/`blueprint_json` 的既有语义。
- 不把 `course_source_plan_json` 当读侧实时真相：它是生成时快照；KB 变更不自动重算。

## 验收

- 迁移测试：旧库升级后旧课程可读；新课程单列可读；非法 JSON 写入失败。
- 事务测试：导入中途失败/取消 → 课程行与 modules/lessons 零残留。
- 读侧测试：`CourseDetail`/`CourseSummary` 对 NULL/旧课程/新课程/未知 version 四态正确；描述流不出现 `source_plan`。
- `cargo test -p nomifun-db`、`cargo test -p nomifun-learning` 全绿；`cargo check --workspace` 干净。
- owner 签核记录写在票面 Resolution（迁移编号、列名、回滚方案）。

## 备注

- AGENTS.md 明确“Database migrations 是高危区，先问再动”；本票在 owner 签核前不进入实现。
- 迁移编号可能被其他分支占用；实施时以 `migrations/` 最大号 +1 为准，并在票面记录实际编号。
- 不把 inventory 全文塞 DB；需要审计时按 path 回 KB 读。
