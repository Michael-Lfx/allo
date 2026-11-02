---
id: x19
title: "format_version 懒再生阀门——可再生产物的格式演进不写迁移"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x19: format_version 懒再生阀门

**What to build:** 节正文/活动等**可再生**产物加 `format_version` 列，打开课时时低于当前版本
即走既有按需生成/重写管线懒升级；把「再生即迁移」从隐性优势变成正式机制。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 按需 + 幂等模式已存在：`learning_lessons.content_generated`（040），课时首开时生成
  （`service/lesson.rs` on-demand 路径）；单节重写 `POST /lessons/{id}/sections/{key}/rewrite`
  （`routes.rs:630` / `service/lesson.rs:1659` / `generation/lesson.rs:532`）；结构
  `regenerate`（`service/learning_graph.rs:342`）；活动 `generate_lesson_activity`。
- 现有「version」列语义：`learning_courses.version`（015）= 课程内容版本计数；
  `learning_lesson_sections.version`（050）= 节重写次数——**都不是格式版本**。
- 可再生清单：structure_json、sections.body_md、activities、mounts、blueprint/samples 旧列。
  不可再生状态不在本票。

## 实施要点

- [ ] 列：`learning_lesson_sections.format_version INTEGER NOT NULL DEFAULT 0`、
      `learning_activities.format_version` 同——两表**已发布**（050）→ **追加**
      `ALTER TABLE ADD COLUMN`，历史行 0 = 待升级。structure 用信封已有 version，不加列。
- [ ] 常量：`SECTION_FORMAT_VERSION`/`ACTIVITY_FORMAT_VERSION` 在生成模块单源；生成/重写
      时打当前值。
- [ ] 懒升级：课时详情读取时 `format_version < CURRENT` → DTO 带 `stale_format=true`；前端
      显示「内容格式待更新」+ 一键重写（复用 degraded 横幅/AI 重写入口 `LessonStudy.tsx:548`
      形态）；后台自动排队可选（走 generation_tasks，受 `NOMI_LEARNING_TASK_GATE`）——
      推荐**用户触发 + 自动排队开关**。
- [ ] 与 x17（visual 码）/x08（结构词表）配合：落地时 bump 对应 format_version，作为第一次
      真实使用。
- [ ] 文档：`CONTEXT.md`/`data-and-storage.md` 加「可再生产物格式演进 = bump 常量 + 懒再生；
      迁移只给不可再生状态」。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db`；`bun run typecheck`；真机：人工把一节
  `format_version` 置 0 → 打开课时出现待更新提示 → 重写后为 CURRENT。

## 非目标

- 不可再生状态的版本（x09）。
