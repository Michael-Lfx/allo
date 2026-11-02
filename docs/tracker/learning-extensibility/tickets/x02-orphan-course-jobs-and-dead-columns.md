---
id: x02
title: "孤儿表 learning_course_jobs 与死列清理——不把零引用的约束带进新版"
type: task
status: closed
assignee:
blocked_by: [x21]
part_of: ../map.md
---

# x02: 孤儿表 learning_course_jobs 与死列清理

**What to build:** 在 x21 基线文件里退役生产代码零读写的 `learning_course_jobs` 表，清掉零引用
死列，同步契约登记、测试与架构文档。

## 现状（2026-11-01 实码核实）

- `learning_course_jobs`（**已发布** `037_learning_course_jobs.sql`：status 9 态 CHECK、
  `kb_id NOT NULL`、`request_json` 无 json_valid）：全仓非测试引用仅
  `nomifun-db/src/id_schema_contract.rs:73,180,296-297,927-929`（契约登记）；无路由、无 service、
  `ui/src` 零引用（`CourseJobTable`/`useCourseJobs`/`course-jobs` 全部不存在）。旧课程 job 相位
  `queued/sampling/blueprint/lessons/importing/…` 只活在这张表的 CHECK 里；现役长任务走内存
  `generation_tasks` 注册表（j03）。
- 死列（代码零引用，实现会话 grep 复核含 smoke/测试）：
  - `learning_courses.graph_meta_json`（已发布 `048_learning_graph.sql`）
  - `learning_lesson_progress.extra_json`（已发布 048 重建）
- 代码已不认的 CHECK 取值：`generation_mode` 的 `'full'`（已发布 040）——
  `CourseGenerationMode`（`models.rs:267`）仅 `OnDemand`。已发布且无害，**只记档 x20**。
- 文档漂移：`docs/architecture/learning.zh.md:208` 仍列 `/api/learning/course-jobs*`，
  `:240-243` 仍列 `CourseJobTable`/`useCourseJobs`。

## 裁决（2026-11-02 grilling Q9 + 影响面核查）

- **直接 DROP，不走 RENAME 观察期。** 依据：表自 j03 注册表接管后零消费者；
  `backup_bundle.rs:953-967` 还原按 `PRODUCT_TABLES` 枚举表（被删表静默跳过）；还原契约
  `validate_current_migration_lineage` 要求 bundle 为**当前完整谱系**——跨版本还原本就
  fail-closed，删表删列不新增还原风险。
- **ADR-0011**：`source_kb_id`↔`source_kb_ids[0]`、`source_path`↔`source_paths_json[0]` 双写
  是**已接受的刻意决定**（单列是 v3 契约兼容位、不退役）。本票不动；x20 登记时引用
  ADR-0011，不记为「未登记不变量」。

## 实施要点

- [ ] 基线文件新增「退役 course_jobs」节：`DROP TABLE learning_course_jobs`（037 已发布 →
      由 x21 基线承载）；`id_schema_contract.rs` 四处登记删除；
      `tests/learning_course_jobs_migration.rs` 改为「表已退役」断言。
- [ ] 死列：基线内 `ALTER TABLE … DROP COLUMN`（需 SQLite ≥ 3.35，确认 sqlx bundled 版本）；
      若 x05 重建 `lesson_progress`，`extra_json` 在重建时自然消失。先 grep 确认零引用。
- [ ] 文档：learning.zh.md 两处；data-and-storage.md 迁移计数由 x21 顺手。

## 验收

- `cargo test -p nomifun-db`（含 id_schema_contract 全表契约）、`cargo check --workspace`；
  `rg learning_course_jobs` 仅剩迁移历史与退役断言；Q8 fixture（含 course_jobs 行）升级通过。

## 非目标

- `course_kind=learning_graph` 不是死值（现役概念网课程仍用它），不动。
- 枚举集 CHECK 治理（x05）。

## Resolution

**已关（2026-11-02，本提交）**。

- **孤儿表退役**：基线 §11 `DROP TABLE learning_course_jobs`；`id_schema_contract.rs`
  六处登记删除（PRODUCT_TABLES / UUIDV7 / NON_REFERENCE 两行 / text_ref 三行）；
  `tests/id_schema_contract.rs` 表数 125→124。
- **死列**：基线 §10 重建 `learning_lesson_progress` 时不再拷贝 `extra_json`，
  重建 `learning_courses` 时不再拷贝 `graph_meta_json`（两列零引用，grep 复核）。
- **测试**：`learning_course_jobs_migration.rs` 整文件改为退役断言（全链后
  `sqlite_master` 无该表）；Q8 fixture 追加两条死列消失断言。
- **文档**：`learning.zh.md` 的 `/api/learning/course-jobs*` 与
  `CourseJobTable`/`useCourseJobs` 两处移除；`models.rs` 的
  `generation_mode` 注释改记「随表退役」。
- **验收**：`cargo test -p nomifun-db` 全绿（含 id_schema_contract 全表契约与 Q8
  fixture）；`rg learning_course_jobs` 仅剩迁移历史、退役断言与契约删除注释。
- 偏差：无（ADR-0011 双写不变量未动）。
