---
id: x11
title: "WS phase 单源 + pipeline 显式字段——前端不再嗅探 triage_* 猜管线"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# x11: WS phase 单源 + pipeline 显式字段

**What to build:** 生成/生长进度帧的 `phase` 取值集收成 Rust 单源常量，帧与课程行显式携带
`pipeline`（`sampling | source_plan`）；前端 phase 处理集中到带 default 的查表函数。

## 现状（盘点 2026-11-01，行号实现会话复核）

- 后端 19 个 phase 字面量散在 `service/generate.rs:130-498`、`service/lesson.rs:291-1828`、
  `service/learning_graph.rs:200-5049` 的 `json!({"phase": "..."})`；`events.rs:46` 注释只列
  7 个；无枚举、无常量表。
- 前端 `ui/src/common/adapter/ipcBridge.ts:1778-1840` 手抄 `phase` union（course 12、
  lesson 6）；`pages/learning/model.ts:192-218` `isModernPipeline` **嗅探是否出现 `triage_*`**
  决定走 `deriveStep` 还是 `deriveModernStep`；分支散在
  `CourseGenerationProgress.tsx:89-132,168-317`、`CourseGenerationPill.tsx:25-67`、
  `LessonStudy.tsx:725-741`、`useCourseLearning.ts:58-62`（`task.state` 比较 string）。
- i18n `genStep*` ×8 + `genBg*` ×4 按 phase 落 key。
- 新增一个 phase 要改：发射点 + ipcBridge union + model.ts 两套折叠 + 两个进度组件 + i18n
  两份——没有中心清单。

## 实施要点

- [ ] Rust：`events.rs` 定义 `pub mod phase { pub const STARTED: &str; … pub const ALL }`
      按 course/lesson/graph 三组；全部发射点改引用常量；测试断言发射点集合 ⊆ `ALL`
      （smoke 读数或 grep 式单测）。
- [ ] 帧字段：`pipeline: "sampling" | "source_plan"`（2026-11-02 Q6 裁决：词汇表「来源结构计划
      Source Plan」对齐，不用 v1/v2），后端按 `NOMI_LEARNING_SOURCE_PLAN`
      （`service/generate.rs:43`）与知识库流判定**显式写出**；课程详情 DTO 同步带
      `pipeline`（有 `course_source_plan_json` 即 `source_plan`）。
- [ ] 前端：删 `isModernPipeline` 嗅探改读字段；`phaseMeta(phase): {step, label, tone}`
      单文件查表 + default（未知 phase → 「处理中」中性态，不丢帧）；`ipcBridge` union 改
      `string`（phase 是开放取值集，由 phaseMeta 兜底）。
- [ ] x12 的互钉脚本把 Rust `phase::ALL` 与 `phaseMeta` 键集对拼——本票先把两端单源化。
- [ ] 文档：`docs/architecture/learning.zh.md` WS 段改引用常量清单。

## 验收

- `cargo test -p nomifun-learning`；`bun run typecheck && bun run check:i18n`；
  真机：v1（描述流）与 v2（知识库流）建课进度条各正确，注入未知 phase 帧不白屏。

## 非目标

- 任务注册表状态机（`generation_tasks.rs:62-66`，内存，已单源）。
