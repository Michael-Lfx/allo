---
id: x05
title: "增长型枚举集列去 CHECK——在 x21 基线内一次移除，胜过以后每加一个值重建一次"
type: task
status: closed
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

## Resolution

**已关（2026-11-02，本提交）**。

- **已发布段（基线 §10，保 id 式重建）**：`learning_activities`、`learning_lesson_sections`、
  `learning_review_events`、`learning_review_log`、`learning_question_tags`、
  `learning_lesson_progress`、`learning_courses` 七表重建；只删枚举集 CHECK 与
  progress 的跨列形态 CHECK，其余列定义（UUID7、json_valid、NOT NULL/DEFAULT、
  UNIQUE、数值域）与索引逐字保留。列清单按运行期 `sqlite_master` 实码取，避免手抄
  漂移。
- **未发布段（基线 §1/§2/§5/§6/§7 原地改）**：`custom_questions.kind`、
  `lesson_concepts.role/tier`、`growth_batches.status`、`xp_events.kind/basis` 与七条
  跨列规则、`findings.source`、`repairs.result`、`quiz_rounds.status` 与三条形态规则
  CHECK 全部移除；`quiz_rounds` 的 partial UNIQUE 逐字保留。
- **Rust 写路径**：`quiz.rs` 新增 `validate_round_shape`（INSERT active / UPDATE
  abandoned / UPDATE completed 三调用点，单测合法 3 态 + 非法 4 样）；
  `progress.rs` 新增 `validate_progress_shape`（upsert 前调用，单测合法 5 态 +
  非法 4 样）；`settle.rs` 的 `validate_settlement` 既有单测覆盖被删的 10 组违约。
- **测试翻面**：sections 文件 3 处、xp_events 1 处（改为保留数值 CHECK 正反样 +
  条款外值可写）、quiz_rounds 1 处、concept_web 2 处；新增 Q8 fixture 永久测试
  `learning_domain_baseline_migration.rs`（v1.7.0 种子 → 基线 → 行数守恒、九种 kind
  存活、约束外取值可写、JSON/数值 CHECK 仍拒）。
- **验收**：`cargo test -p nomifun-db` 全绿（49 个测试二进制）；
  `cargo test -p nomifun-learning` 740 绿；`learning-smoke xp` / `quiz` EXIT=0
  （零 LLM 档）；`content` 档需真机凭据，本会话未跑，留待波次真机批。
- 偏差：`learning_sections_migration.rs` 的夹具从「015→050 局部链」改为全链
  （MIGRATOR.run）——终态断言必须跑 x21 基线，局部链缺 052 等表会失效（图纸 §8 记档）。
