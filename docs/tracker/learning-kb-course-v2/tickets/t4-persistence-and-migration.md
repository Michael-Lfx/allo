---
id: t4
title: "持久化：单个 course_source_plan_json 快照列 + 迁移（owner 签核）"
type: task
status: closed
assignee: zhangyj
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

**Status:** closed（Resolution 见文末）

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

## Resolution（2026-10-21 关票）

**签核记录**（高风险区要件）：迁移编号 **069**、列名 **`learning_courses.course_source_plan_json`**（TEXT、nullable、无默认值、不回填旧行）。签核依据 = owner 在 map Q14 的原则批准（单列方案、Grill 两轮定案）+ 2026-10-21 的明示执行指令（以冻结形状实施、不得偏离）；形状与票面一致，未偏离。**回滚方案 = 读侧忽略新列**（迁移只加列，不删数据、不改写 068 及既有列；列可留在库里不动）。owner 推送前可否决。

**交付**（分支 `learn-v2/t4-persistence`，提交 `a82305ec3` 迁移 + `654950764` 信封 + `976cf31b4` 写读接线）：

- **迁移 069**：`migrations/069_learning_course_source_plan.sql` —— `ALTER TABLE learning_courses ADD COLUMN course_source_plan_json TEXT` + `CHECK (NULL OR (json_valid AND json_type='object'))`（仿 068 `samples_meta_json`）；契约测试 `tests/learning_course_source_plan_migration.rs`（旧行 NULL 可读且既有列不受扰、JSON object 原样读回、数组/标量/非法 JSON 被 CHECK 拒绝）。**契约登记不动**：只加非引用语义的 JSON 快照列，先例（`samples_meta_json`/`graph_meta_json`/`structure_json`）均不在 `PRODUCT_TABLES`/`LOGICAL_REFERENCES`/`JSON_LOGICAL_REFERENCES` 登记表里；新表才需要登记。
- **信封**：`CourseSourcePlanSnapshot { version: 1, inventory, triage, plan, coverage, warnings }`，住 `generation/snapshot.rs`、经 lib.rs 再导出；字段用 t1–t3 冻结类型（`KbInventory`/`KbTriageReport`/`SourcePlan`/`SourceCoverage`），版本号供未来演进——读侧未知版本降级 `available=false`，不崩溃不伪造。信封级 `warnings` 承载快照截断披露（票面「记进信封或快照 warnings」二选一的后者）。
- **体积控制**（`capture()` 序列化入口统一截断，**不改 t1–t3 内存类型语义**，测试钉死原值不动）：preview ≤200 字（内存口径 512）、单文件 headings ≤16 条（内存 32），截断事件聚合成信封 warnings 不静默；plan/coverage 本就只含结构化骨架与 ≤120 字锚点片段（无 lesson 正文，审计按 `rel_path` 回 KB 读全文）；1000 文件库快照 <2MB（测试钉）。
- **写侧**：`OutlineSnapshot` 增 `course_source_plan_json: Option<String>`；`import_course_outline` 与 modules/lessons **同一事务** INSERT（中途失败/取消零残留，CHECK 违约走公共接缝实测：courses/modules/lessons 三表 COUNT=0）；`generate_course` 接线——只在「计划 → 展开」成功（inventory/triage/plan/coverage 四件套齐全）时于落库点组装信封；描述流/回退采样路径/legacy → NULL（不伪造快照）。
- **读侧最小视图**：`CourseSummary.source_plan: Option<CourseSourcePlanView>`（`CourseDetail` 内嵌 course 摘要自动携带）：`{version, available, triage 七角色计数, coverage 四级计数(chapters/lessons/files/topics)}`；四态实测：旧课程 NULL → 字段不出现；新课 → available=true 带计数；未知 version（99）→ available=false 不报错；v1 形状损坏 → available=false 计数置零。解析仿 `parse_sampling_view`（诊断字段不让列表整体失败）。DTO 细化归 t5。
- 测试：`cargo test -p nomifun-db` 全绿（458+ 迁移/契约各套 0 failed）；`cargo test -p nomifun-learning` **430 passed / 0 failed**（基线 423 + 新增 7：信封 2 + 写侧 1 + 接线 2 + 读侧 1 + 事务 1）；`cargo check --workspace` 干净（新代码零警告）。

**实施偏差**（不违背票面）：

1. 快照只在展开成功时写：t2 计划可用但 t3 展开失败回退大纲引擎的课程，列上 NULL——课程不是按 plan 建成的，快照不得冒充（「无 plan 写 NULL」的自然推论，实测钉死）。
2. 读侧视图挂 `CourseSummary` 一处（详情页内嵌同一摘要）而非 Summary/Detail 各加一份——最小可用，t5 细化 DTO 时再分面。
3. 迁移测试聚焦列契约（NULL/对象/非法拒绝）与导入事务原子性（服务层公共接缝）；「1000 文件可控」以序列化体积上界测试承担，未建 1000 文件真库（夹具同口径）。
