---
id: x05
title: "增长型枚举集列去 CHECK——在 x21 基线内一次移除，胜过以后每加一个值重建一次"
type: task
status: open
assignee:
blocked_by: [x01, x21]
part_of: ../map.md
---

# x05: 增长型枚举集列去 CHECK

**What to build:** 对「历史上已多次因扩值重建、预期还会增长」的枚举集列去掉 `CHECK IN(...)`，
校验收口到 Rust 单源 enum（ADR-0015）；纯数值域/布尔/json_valid CHECK 保留。全部在 x21 基线
文件内完成。

## 现状（盘点 2026-11-01，行号实现会话复核）

SQLite 改 CHECK = 复制整张表。这些列已被重建过：`activities.kind` 3 次、
`custom_questions.kind` 4 次、`lesson_progress.status` 2 次、`review_items` 1 次。

**已发布段（基线内重建：新表 → INSERT SELECT → DROP → RENAME；升级时搬用户数据，Q8 fixture 必测）：**

| 列 | 迁移 slug | 增长理由 |
| --- | --- | --- |
| `learning_activities.kind` | 050_sections_and_question_kinds | 题型 9 种还在加 |
| `learning_lesson_sections.kind` / `.status` | 050 | 节类型、节状态 |
| `learning_review_events.source` | 042_checkins | 封死 `course\|custom`，quiz 已是第三来源（x13） |
| `learning_review_log.source` / `.rating_source` | 052_review_log | 同上 / auto·self·synthetic |
| `learning_question_tags.source` | 036_tags | 同上 |
| `learning_lesson_progress.status` + 跨列形态 CHECK | 048_learning_graph | 状态机（x02 的 `extra_json` 顺带消失） |
| `learning_courses.course_kind` / `.teaching_style` | 048 / 050 | 课程类型与风格会加 |

**未发布段（基线内直接 CREATE 终态，不带 CHECK）：**

| 列 | 原迁移 slug |
| --- | --- |
| `learning_custom_questions.kind` | 069_retire_graph_dag（x01 先修回退，折叠带入） |
| `learning_xp_events.kind` + `.basis` + 7 条跨列规则 CHECK | 081_xp_events |
| `learning_quiz_rounds.status` + 3 条形态 CHECK | 084_quiz_rounds |
| `learning_lesson_concepts.role` / `.tier` | 070_concept_web_growth（x08 tier 取 apply） |
| `learning_growth_batches.status` / `learning_structure_findings.source` / `learning_structure_repairs.result` | 071 / 082 / 083 |

**保留**：`score 0..1`、`rating 0..4`、`stability_days>=0`、`difficulty 1..10`（FSRS 域，
x10 记档）、`review_day` 范围、布尔 `CHECK(0,1)`、`json_valid`、`trim(...)<>''`。

## 为什么与 v3 契约一致

v3 已选「无物理 FK/触发器，靠 `id_schema_contract.rs` 运行时登记」；枚举集校验移到代码层是
同一哲学（ADR-0015）。失去的 DB 最后防线由 x12 的互钉脚本 + 写路径严格 `TryFrom` 补回。

## 实施要点

- [ ] 已发布段：在基线对应节重建，索引/UNIQUE 逐字保留；**Q8 fixture 从 v1.7.0 形状种子跑到
      尖端**，行数守恒、约束外取值可写入、数值域 CHECK 仍生效。
- [ ] 未发布段：改基线 CREATE；`quiz_rounds` partial UNIQUE
      `(user_id, course_id) WHERE status='active'` 逐字保留。
- [ ] 跨列规则 CHECK（xp_events 7 条、quiz_rounds 3 条、lesson_progress 形态）：移到 Rust 写
      路径 `validate()`，每条原规则一个单测。
- [ ] 写路径：所有落库 enum 的 `as_str()` 为唯一写入口；`TryFrom<&str>` 仍严格拒未知（读宽容
      在 x12）。
- [ ] `id_schema_contract.rs` 若登记了 CHECK 取值（复核）同步。
- [ ] 文档：`data-and-storage.md` 加「枚举集列不设 CHECK」纪律（指 ADR-0015）。

## 验收

- `cargo test -p nomifun-db -p nomifun-learning` 全绿；`learning-smoke quiz/xp/content` 档
  EXIT=0（写读闭环）；Q8 fixture 升级通过且行数守恒。

## 非目标

- 新增任何枚举值（x13 扩 quiz 单独票）；读路径 Unknown 兜底（x12）。
