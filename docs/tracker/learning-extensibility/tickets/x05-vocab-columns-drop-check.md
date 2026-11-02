---
id: x05
title: "增长型词表列去 CHECK——随新版升级一次重建，胜过以后每加一个值重建一次"
type: task
status: open
assignee:
blocked_by: [x01]
part_of: ../map.md
---

# x05: 增长型词表列去 CHECK

**What to build:** 对「历史上已多次因扩词重建、预期还会增长」的词表列去掉
`CHECK IN(...)`，校验收口到 Rust 单源 enum；纯数值域/布尔/json_valid CHECK 保留。

## 现状（盘点 2026-11-01，行号实现会话复核）

SQLite 改 CHECK = 复制整张表。这些列已被重建过：`activities.kind` 3 次、
`custom_questions.kind` 4 次、`lesson_progress.status` 2 次、`review_items` 1 次。

**已发布段（追加重建迁移，升级时搬用户数据，需 fixture 测试）：**

| 列 | 迁移 slug | 增长理由 |
| --- | --- | --- |
| `learning_activities.kind` | 050_sections_and_question_kinds | 题型 9 种还在加 |
| `learning_lesson_sections.kind` / `.status` | 050 | 节类型、节状态 |
| `learning_review_events.source` | 042_checkins | 封死 `course\|custom`，quiz 已是第三来源（x13） |
| `learning_review_log.source` / `.rating_source` | 052_review_log | 同上 / auto·self·synthetic |
| `learning_question_tags.source` | 036_tags | 同上 |
| `learning_lesson_progress.status` + 跨列形态 CHECK | 048_learning_graph | 状态机 |
| `learning_courses.course_kind` / `.teaching_style` | 048 / 050 | 课程类型与风格会加 |

**未发布段（原地改创建迁移）：**

| 列 | 迁移 slug |
| --- | --- |
| `learning_custom_questions.kind` | 069_retire_graph_dag（x01 先修回退） |
| `learning_xp_events.kind` + `.basis` + 7 条跨列规则 CHECK | 081_xp_events |
| `learning_quiz_rounds.status` + 3 条形态 CHECK | 084_quiz_rounds |
| `learning_lesson_concepts.role` / `.tier` | 070_concept_web_growth（x08 要统一 tier） |
| `learning_growth_batches.status` / `learning_structure_findings.source` / `learning_structure_repairs.result` | 071 / 082 / 083 |

**保留**：`score 0..1`、`rating 0..4`、`stability_days>=0`、`difficulty 1..10`（FSRS 域，
x10 记档）、`review_day` 范围、布尔 `CHECK(0,1)`、`json_valid`、`trim(...)<>''`。

## 为什么与 v3 契约一致

v3 已选「无物理 FK/触发器，靠 `id_schema_contract.rs` 运行时登记」；词表校验移到代码层是
同一哲学。代价是失去 DB 层对脏写的最后防线——用 x12 的互钉测试 + 写路径严格 `TryFrom` 补回。

## 实施要点

- [ ] 已发布段：追加迁移，每张表一段「新表→INSERT SELECT→DROP→RENAME」，索引/UNIQUE
      逐字保留；**fixture 测试从 v1.7.0 形状种子跑到尖端**（map 裁决点 1）。
      x02 若选重建 lesson_progress 则 `extra_json` 顺带在此处理。
- [ ] 未发布段：原地改创建迁移；`quiz_rounds` partial UNIQUE
      `(user_id, course_id) WHERE status='active'` 逐字保留。
- [ ] 跨列规则 CHECK（xp_events 7 条、quiz_rounds 3 条、lesson_progress 形态）：移到 Rust
      写路径 `validate()`，每条原规则一个单测。
- [ ] 写路径：所有落库 enum 的 `as_str()` 为唯一写入口；`TryFrom<&str>` 仍严格拒未知
      （读宽容在 x12）。
- [ ] `id_schema_contract.rs` 若登记了 CHECK 词表（复核）同步。
- [ ] 文档：`docs/architecture/data-and-storage.md` 加「词表列不设 CHECK」纪律。

## 验收

- `cargo test -p nomifun-db -p nomifun-learning` 全绿；`learning-smoke quiz/xp/content` 档
  EXIT=0（写读闭环）；v1.7.0 形状 fixture 升级通过且行数守恒。

## 非目标

- 新增任何词表值（x13 扩 quiz 单独票）。
- 读路径 Unknown 兜底（x12）。
