---
id: g4
title: "生长全局悬浮进度 Pill——推广建课链 CourseGenerationPill 模式"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# g4: 生长全局悬浮进度 Pill

**What to build:** 复用 `CourseGenerationPill` 骨架
（`fixed right-24px bottom-24px z-100`，挂 index.tsx:458），为 growth 做
全局悬浮折叠进度条：跨页可见（离开图工作区不失联）、点击回图工作区、
运行中可取消。状态纪律与建课链同款：**轮询为准（generation-tasks 快照
端点）、WS 事件仅增强**（不重放不补发）。后端素材全现成：`set_step`
逐步文案 + `emit_graph_event`（started/round/redraft/failed）+ cancel token。

**验收：**
- growth 运行中切任意页面 Pill 仍在且进度继续；终态以 HTTP 为准
- 新文案 zh-CN/en-US 双语对称；`bun run check:theme` + `check:dead-css` 过

**开放点：** lesson 任务是否并入 Pill（建议二期，lesson 已有 accepted+poll 面）。

**施工记录（code-review 后补记 2026-11-10）：** 三处票外裁量，均判合法：
①新建 `learningFloatingDock.ts` 宿主并把 `CourseGenerationPill` portal 化——两 Pill 同角落必然重叠，宿主缺席时回退原 fixed 定位；②`useCourseCreation.refreshGenerationStatus` 数据源由 legacy `generationStatus()` 换 `generationTasks()` 按 `kind !== growth` 过滤——防建课 Pill 把 growth 任务误认领成双 Pill 展示，副作用=建课 Pill 不再认领 growth（与全局 Pill 自洽），「首个 running ?? 首个 queued」与 legacy「queued 计数+最新主题」语义略有出入（可接受）；③挂载点 ProtectedLayout（Router.tsx）而非票面字面 index.tsx:458——达成「切任意页面仍在」验收之必要偏离。**遗留（票外现存，未修）**：建课 Pill 的取消按钮仍调旧的全量取消端点，会连带取消正在跑的 growth——列为后续票候选（见 map 开放问题）。

**病历依据：** 用户裁决（2026-11-09）：生成后台执行+悬浮折叠进度随时查看；
growth 948s 全程单页面干等。

## Resolution

已关（2026-11-10）：commit `e01674fca`，并回 feat/learn0928。全局 Pill（ProtectedLayout 挂载、轮询为准+WS 增强、取消/回跳、结果驻留）；12 条新测试，133 学习 UI 全绿；i18n/theme 零新增失败。票外裁量与遗留已补记本票（见施工记录）。
