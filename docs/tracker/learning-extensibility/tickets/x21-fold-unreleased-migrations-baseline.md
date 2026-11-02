---
id: x21
title: "未发布学习迁移链折叠为单一净形基线——纯重构，零 schema 变化"
type: task
status: open
assignee:
blocked_by: [x01]
part_of: ../map.md
---

# x21: 未发布学习迁移链折叠为净形基线

**What to build:** 把未发布学习链 `069_retire_graph_dag_and_concepts`–`088_learning_basis_declarations`
（20 文件、810 行）折叠为一个 `0NN_learning_domain_rebuild.sql`。**纯重构**：最终 schema 与旧
20 段链逐项等价（表/列/索引/CHECK），零行为变化；此后 x-wave 全部迁移票只编辑本文件。

## 先例与编号

- 先例 `217030a7c fix(db): fold the learning migration chain into one version past main's 066/067`
  曾折成 `068_learning_domain_rebuild.sql`，后被吸收波合并拆回。slug 复用 `learning_domain_rebuild`
  （文档 `learning.zh.md:181` 本就这么叫），不用 `_v2`（词汇表里 v2 被脏库路径占用）。
- 号 = 折叠时 main 尖端最大号 + 1（main 现在到 `069_marketplace_index_source_kind`，本分支只有
  `069_retire…`，合并即撞号）→ 今天是 070；最终 rebase 若 main 又长则改号，单文件零成本。

## 已核实的折叠清单（2026-11-02 实码核实）

**必须逐字保留并按原序排列（作用于已发布表的数据变换）：**
- 069：14 条级联 DELETE（清旧 DAG 图课程在 review_log/question_tags/review_items/attempts/
  activity_concepts/activities/sections/progress/lessons/enrollments/modules/course_tags/
  graph_prerequisites/courses 的数据）→ DROP 六张旧表（graph_prerequisites/mastery_states/
  activity_concepts/**lesson_concepts(015 旧语义)**/concept_prerequisites/concepts）→
  custom_questions 重建（**带 x01 的 kind 全集修复**，或按 x05 去 CHECK）。
  `lesson_concepts` 是「DROP 015 旧表 → CREATE 070 新语义表」两步，都保留。
- 076 `source_kb_ids` 回填 UPDATE；078 `source_paths_json` 回填 UPDATE。
- 对已发布表 `learning_courses`/`learning_lessons` 的 ALTER ADD（072 deleted_at、073 difficulty、
  074 structure_json、075 samples_meta_json、076 source_kb_ids、077 JSON 索引、078 source_paths_json、
  079 enabled、086 course_source_plan_json）按原序。

**写净形消掉（未发布表自己的建-改-删折腾）：**
- 070 建 `learning_course_endpoints` + `compass_md`/`compass_updated_at` → 074 DROP（连带 074
  两条端点课时 DELETE——用户库无端点表，本就空操作）。
- 071 补 `growth_batches.status` + partial 索引、080 aliases_json 改形 UPDATE + `lesson_concepts.ref_name`、
  085 补 `concept_registry.source` → 直接建终态。

**文件结构：** 分节注释，每节标原 slug；每节保留原文件头的 `v3 contract:` 自述一行；文件头注明
「折叠自 069_retire…–088（先例 217030a7c）；净形：未发布表建终态、已发布表变换逐字保留」。

## 实施要点

- [ ] 等价校验（一次性）：同一 v1.7.0 形状种子（Q8 助手：跑 ≤068 → 播种），分别跑旧 20 段链与
      新基线各建一库；比对规范化 `sqlite_master`（表/列/索引/CHECK 逐条）+ 全部学习表数据逐行
      相等。脚本一次性运行，结果（表数/行数/差异=0）记进本票 Resolution，不留永久测试。
- [ ] 存量 15 个迁移测试：14 个走 `sqlx::migrate!` 全链原样通过；
      `tests/learning_concept_web_migration.rs` 的 6 处 `include_str!` 改指基线。
- [ ] `id_schema_contract.rs` / `tests/id_schema_contract.rs`：表集合不变，理论上零改动；复核确认。
- [ ] 文档：`learning.zh.md` 迁移族节、`data-and-storage.md`（顺手「45 个迁移」→「见目录」+
      Q22 政策一行）、`docs/contracts/*` 的迁移号引用改 slug。ADR 历史文本不改。
- [ ] dev 库后果写进提交信息与 CONTEXT.md 开发注记：折叠后 dev 库谱系非前缀、启动即拒
      （`validate_current_migration_lineage`），`bun run reset:dev` 重建；smoke 临时库不受影响。
- [ ] 本票之后 x02/x05/x06/x07/x08/x10/x16/x19 一律编辑本文件对应节（串行取号不再需要——
      只有一个文件）。

## 验收

- `cargo test -p nomifun-db` 全绿（含契约测试与重指后的 concept_web 测试）；等价校验差异=0
  记档；Q8 fixture 从 v1.7.0 形状升级通过且行数守恒；`cargo check --workspace`。

## 非目标

- 任何 schema/行为变化（x-wave 各票做）；已发布段折叠或改动（永不）。
