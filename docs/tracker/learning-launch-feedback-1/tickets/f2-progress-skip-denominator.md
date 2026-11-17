---
id: f2
title: 课程进度分母含 skipped → 进度永不可能满
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

课程卡进度 0/19：跳过的节点既不进分子也不出分母，进度永远不可能到满。
口径应该怎么改？

## 已核验事实（2026-11-17 dev 库实证 + 对码）

- **库态实证**（dev 库，课程 01a1e364，19 课时）：`learning_lesson_progress`
  中 15 行 `status='skipped'`、0 行 `completed`、4 课无 progress 行 → 卡片
  0/19。即使做完剩余 4 课，进度恒 ≤ 4/19 = 21%。
- **SQL 口径**（`service/course.rs:226-233` list_courses、`:280-287`
  course_detail）：
  - `total_lessons` = 课程下**全部**课时数（不看 progress 状态）；
  - `completed_lessons` = 只数 `p.status = 'completed'`。
- **skip 语义**（`service/progress.rs:77-85`）：skipped 不伪造 started_at、
  **永不写 completed_at**（CHECK 约束要求 NULL）→ 数学上不可能进分子。
  skip 可逆（图工作区跳过/取消跳过切换，LearningGraphWorkspace.tsx:992-997
  → `not_started`）。
- **前端消费点**（施工时逐点列全，至少）：课程卡 `CourseCard`（截图
  「进度 0/19 课时」）、传统工作区大纲头（CourseWorkspace.tsx:296-302
  `completed/total` + Progress）、图工作区大纲头同族读数。

## Terms（口径三选一，票内荐 A）

- **A（荐）：分母剔除 skipped**——`progress = completed / (total − skipped)`。
  「进度 = 实际学会占比」语义诚实；unskip 自动回分母；全跳时光分母 0 →
  显示「—」（不渲染 0/0）。改动集中分母计算处（建议后端单源加
  `skipped_lessons` 读数，前端两处消费跟随），skip 流转已有测试网。
- **B：skipped 算分子**——与「完成」语义冲突（没学过算完成）；且 skip 可
  逆，完成数会随取消跳过回退，「已完成 N 课」变成可逆数字。不荐。
- **C：双读数不改分母**——「已完成 a · 已跳过 b / 共 N」。信息全但卡片
  数字失去「进度」含义，且 19 分母的「永不可能满」仍在。作 fallback。

## 裁决（2026-11-17 grilling）

- **选 A**：分母剔除 skipped（progress = completed / (total − skipped)）；
  全跳课程分母 0 → 显示「—」、不渲染进度条；unskip 自动回分母。B（算分
  子）/ C（双读数）按 Terms 理由否决。
- **后端单源**：`CourseSummary`（models.rs:1955）加 `skipped_lessons`；
  `list_courses`（course.rs:226-233）与 `course_detail`（:280-287）两处 SQL
  各加 skipped 子查询；前端类型（ui types.ts:257-258 邻位）加字段，分母由
  前端派生（total − skipped，钳 ≥0）。
- **消费点全集（2026-11-17 对码）**：`CourseCard.tsx:110-112`（percent，
  分子 completed_lessons）+ `:194`（计数行，分子 settled_or_completed——
  两处分母同口径剔除 skipped）；`CourseWorkspace.tsx:90` + `:299`。图工作区
  无 completed/total 读数（grep 无命中），不涉及。

## Acceptance

- 后端口径单源：分母 = total − skipped（或裁决结果），`nomifun-learning`
  单测覆盖：skip 1/3 → 分母缩 1；unskip → 回分母；全 skip → 分母 0 分支。
- 前端全部消费点跟随同一口径，双 locale 文案（若有措辞变化）。
- 回归：learning 后端 + 前端既有绿；typecheck / i18n 绿。

## Resolution

（未结案）
