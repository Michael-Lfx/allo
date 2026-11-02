---
id: x02
title: "孤儿表 learning_course_jobs 与死列清理——不把零引用的约束带进新版"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x02: 孤儿表 learning_course_jobs 与死列清理

**What to build:** 退役生产代码零读写的 `learning_course_jobs` 表，清掉零引用死列，
同步契约登记、测试与架构文档。

## 现状（2026-11-01 实码核实）

- `learning_course_jobs`（**已发布** `037_learning_course_jobs.sql`：status 9 态 CHECK、
  `kb_id NOT NULL`、`request_json` 无 json_valid）：全仓非测试引用仅
  `nomifun-db/src/id_schema_contract.rs:73,180,296-297,927-929`（契约登记）；
  无路由、无 service、`ui/src` 零引用（`CourseJobTable`/`useCourseJobs`/`course-jobs`
  全部不存在）。旧课程 job 相位 `queued/sampling/blueprint/lessons/importing/…` 只活在
  这张表的 CHECK 里；现役长任务走内存 `generation_tasks` 注册表（j03）。
- 死列（代码零引用，实现会话 grep 复核含 smoke/测试）：
  - `learning_courses.graph_meta_json`（已发布 `048_learning_graph.sql`）
  - `learning_lesson_progress.extra_json`（已发布 048 重建）
- 代码已不认的 CHECK 取值：`generation_mode` 的 `'full'`（已发布 040）——
  `CourseGenerationMode`（`models.rs:267`）仅 `OnDemand`。已发布且无害，**只记档**。
- 文档漂移：`docs/architecture/learning.zh.md:208` 仍列 `/api/learning/course-jobs*`，
  `:240-243` 仍列 `CourseJobTable`/`useCourseJobs`；`docs/architecture/data-and-storage.md`
  写「45 个迁移」实际 88。

## 为什么必修

新版上线后每张表都是「用户数据承载面」，退役要走弃用期；死列要永远在 schema 文档里解释。
用户库里这张表的行是旧管线的历史任务记录，UI 早已不展示。

## 实施要点

- [ ] 037 已发布 → **追加迁移**：map 裁决点 2 选 `DROP TABLE` 或
      `ALTER TABLE … RENAME TO _retired_learning_course_jobs`（下版再删）。
      `id_schema_contract.rs` 四处登记同步删除/改名；
      `tests/learning_course_jobs_migration.rs` 改为「表已退役」断言。
- [ ] 死列：048 已发布 → 追加 `ALTER TABLE … DROP COLUMN`（需 SQLite ≥ 3.35，确认
      sqlx bundled 版本；否则重建——若 x05 重建 lesson_progress 顺带处理 `extra_json`）。
- [ ] 应用层双写不变量 `source_kb_id`↔`source_kb_ids[0]`、`source_path`↔
      `source_paths_json[0]`（`076`/`078` slug 迁移）——**本票只记档**，列入 x20 契约总表
      「应用层不变量」。
- [ ] 文档：learning.zh.md 两处、data-and-storage.md 迁移计数改为「见 migrations 目录」。

## 验收

- `cargo test -p nomifun-db`（含 id_schema_contract 全表契约）、`cargo check --workspace`；
  `rg learning_course_jobs` 仅剩迁移历史与退役断言。
- v1.7.0 形状 fixture（含 course_jobs 行）跑到尖端通过。

## 非目标

- `course_kind=learning_graph` 不是死值（现役概念网课程仍用它），不动。
- 词表 CHECK 治理（x05）。
