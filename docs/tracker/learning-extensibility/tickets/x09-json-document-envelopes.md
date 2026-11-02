---
id: x09
title: "JSON 文档列 v1 信封——不可再生状态优先，复用 CourseSourcePlanSnapshot 模式"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x09: JSON 文档列 v1 信封

**What to build:** 每个落库 JSON 文档列写侧恒打 `version`，读侧按版本分支、未知版本显式降级
——把 `CourseSourcePlanSnapshot` 已证明的模式推广到全部 JSON 列。零迁移。

## 现状（盘点 2026-11-01，行号实现会话复核）

全仓 `schema_version` 零命中。有版本的只有：
- `course_source_plan_json` ← `generation/snapshot.rs:20,29`
  `COURSE_SOURCE_PLAN_SNAPSHOT_VERSION=1`，读侧未知版本 → `available=false`（范本）。
- `structure_json` 外层信封 `{version:1, generated_at, structure, terrain}`，但读侧
  `service/learning_graph.rs:946` 只回显不分支。

无版本、只靠 `#[serde(default)]` 单向兼容（能加字段，不能改语义/删字段）：

| 列 | 结构体 / 解析 | 可再生 | 发布态 | 优先 |
| --- | --- | --- | --- | --- |
| `quiz_rounds.plan_json/answers_json/report_json` | `QuizPlanItem` `service/quiz.rs:73`（`kind: String`）、`StoredRoundReport` :1000 | 否 | 未 | **1** |
| `basis_declarations.doors_json/blocks_json` | `basis_declaration.rs` | 否 | 未 | **1** |
| `concept_registry.aliases_json` | `AliasRecord` `alias_records.rs:20`；080 已做过一次**无版本原地改形** :11-17 | 否 | 未 | **1** |
| `structure_repairs.ops_json` | `StructureOp` `structure_ops.rs:43,245` 手写解析 | 否 | 未 | 1 |
| `attempts.response_json` | 裸 Value（x16 判别） | 否 | **已** | 1 |
| `activities.config_json` | `StoredActivityConfig` `models.rs:3350` 字段逐代累积 default | 是 | **已** | 2 |
| `structure_mounts.mounts_json` | `MountEntry` `mounting.rs:61` 手写解析 | 是 | 未 | 2 |
| `courses.samples_meta_json` / `blueprint_json` / `samples_json`（后两者无 json_valid） | — | 是 | 未/已 | 3 |
| `lessons.source_paths_json` / `growth_batches.node_ids_json` / `courses.source_kb_ids` / `basis_declarations.concept_ids_json` | 纯数组 | — | — | 记档不包 |

## 实施要点

- [ ] 信封形状统一为**扁平**（2026-11-02 Q7 勘误）：`version` 与载荷键并列，不包 `data` 层——
      与现存两信封同形（`generation/snapshot.rs:29-38`、`learning_graph/lexicon.rs:202` 已核实）；
      常量命名 `*_SCHEMA_VERSION` 与 x20 对齐。发版门槛只含优先 1 的学习者状态五列（Q1）。
- [ ] 读侧单源 `read_envelope::<T>(raw, CURRENT) -> Result<Versioned<T>>`：缺 version 视为
      v0（历史裸对象，**已发布列必走此路**），未知高版本 → 显式 `Unsupported`，禁止猜形。
- [ ] 写侧：所有 INSERT/UPDATE 经同一 `write_envelope`；禁止散落 `json!`。
- [ ] 优先 1 五列先落；2/3 随后或拆子提交。数组列不包，记入 x20「纯数组，演进只允许
      追加元素」。
- [ ] 历史裸对象**不回填**（读侧 v0 兼容），与 source plan「旧行 NULL 不回填」同口径。
- [ ] 测试：每列 v0 旧样 → 读通；v1 → 读通；v99 → 显式不支持；写出必带 version。

## 验收

- `cargo test -p nomifun-learning`；`learning-smoke quiz/structure/mount/repair` 档 EXIT=0；
  `rg "version" crates/backend/nomifun-learning/src` 显示每列写点经 helper。

## 非目标

- 任何语义改动（只加壳）；答案判别（x16）；契约总表（x20）。
