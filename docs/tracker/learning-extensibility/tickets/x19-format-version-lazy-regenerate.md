---
id: x19
title: "format_version 懒再生阀门——可再生产物的格式演进不写迁移；接管节排除"
type: task
status: open
assignee:
blocked_by: [x21]
part_of: ../map.md
---

# x19: format_version 懒再生阀门

**What to build:** 节正文/活动等**可再生产物**加 `format_version` 列，打开课时时低于当前代际即
提示并由用户触发既有按需生成/重写管线懒升级；把「再生即迁移」从隐性优势变成正式机制
（ADR-0016 第 2 条）。**接管节**（CONTEXT.md「Taken-over Section」）永不自动再生——需新增
持久化接管标记（影响面核查勘误）。

## 现状（盘点 2026-11-01 + 核查 2026-11-02）

- 按需 + 幂等模式已存在：`learning_lessons.content_generated`（040），课时首开时生成
  （`service/lesson.rs` on-demand 路径）；单节重写 `POST /lessons/{id}/sections/{key}/rewrite`
  （`routes.rs:630` / `service/lesson.rs:1659` / `generation/lesson.rs:532`）；结构 `regenerate`
  （`service/learning_graph.rs:342`）；活动 `generate_lesson_activity`。
- 现有「version」列语义：`learning_courses.version`（015）= 课程内容修订计数；
  `learning_lesson_sections.version`（050）= 节重写次数——**都是修订（revision），不是代际**
  （CONTEXT.md「Generation」；legacy 命名不改）。
- **接管无持久化标记**：`learning_lesson_sections` 列仅 `…status/version/visual/degraded`；
  `service/lesson.rs:1891` 手动编辑路径只清 `degraded`（ADR-0007「手动编辑视为接管」）。
  没有标记就无法把接管节排除在自动再生之外。
- 可再生清单：structure_json（信封已有 version，不加列）、sections.body_md、activities、mounts。

## 裁决（2026-11-02 Q17）

stale_format 只做「格式待更新」提示 + 用户显式触发重写（告知会覆盖手改）；自动排队开关默认关；
**接管节永不进自动再生队列**。

## 实施要点

- [ ] 列（050 已发布 → 在 x21 基线内 `ALTER TABLE ADD COLUMN`，历史行取默认）：
      `learning_lesson_sections.format_version INTEGER NOT NULL DEFAULT 0`、
      `learning_activities.format_version INTEGER NOT NULL DEFAULT 0`、
      `learning_lesson_sections.edited_at INTEGER`（NULL = 未接管；接管时间戳）。
      `id_schema_contract.rs` 非引用列登记同步。
- [ ] 接管落标：`service/lesson.rs:1891` 手动编辑路径在清 degraded 的同时写 `edited_at`；
      DTO 披露 `taken_over: bool`。
- [ ] 常量：`SECTION_FORMAT_VERSION` / `ACTIVITY_FORMAT_VERSION` 在生成模块单源；生成/重写时
      打当前值。
- [ ] 懒升级：课时详情读取时 `format_version < CURRENT && edited_at IS NULL` → DTO 带
      `stale_format=true`；前端显示「内容格式待更新」+ 一键重写（复用 degraded 横幅/AI 重写
      入口 `LessonStudy.tsx:548` 形态）；接管节 `stale_format` 仍可为 true 但文案改「已手动
      编辑，重写将覆盖」且绝不自动排队。自动排队开关 `NOMI_LEARNING_AUTO_FORMAT_UPGRADE`
      缺省关（走 generation_tasks，受 `NOMI_LEARNING_TASK_GATE`）。
- [ ] 与 x17（visual 码）/x08（structure 取值）配合：落地时 bump 对应代际，作为第一次真实使用。
- [ ] 文档：`CONTEXT.md` 开发注记 / `data-and-storage.md` 指向 ADR-0016。

## 验收

- `cargo test -p nomifun-learning -p nomifun-db`；`bun run typecheck`；真机：人工把一节
  `format_version` 置 0 → 打开课时出现待更新提示 → 重写后为 CURRENT；手动编辑过的节开自动开关
  也不被排队。

## 非目标

- 学习者状态的版本（x09）。
