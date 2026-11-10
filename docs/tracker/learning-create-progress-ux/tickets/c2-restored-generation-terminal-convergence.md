---
id: c2
title: "恢复态 running/queued 终态收敛——指示条与弹窗自动翻到 done/failed/cancelled，完成可进入课程"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# c2: 恢复态生成任务的终态收敛

**What to build:** `useCourseCreation.refreshGenerationStatus` 重建出的
running/queued 态（页面切走再回来、或对话框关闭后回页）目前永远停在该态——
补一个收敛机制：恢复态存活期间跟踪对应任务（kind 过滤沿用既有
`!== GROWTH_TASK_KIND`），当快照中该任务翻到终态时，把弹窗与悬浮指示条一并
推到对应终态：

- `done` → 用 `GenerationTaskView.course_id` 拉课程详情 → `completed` + 卡片
  （「开始学习」可跳 `/learn/:id`）；
- `failed` → `failed` + 注册表 detail（文案面归 c3，本票只接现状字段）；
- `cancelled` → `cancelled` 态（既有重试面）；
- 任务从快照消失（注册表清账）→ 按 `cancelled` 收敛兜底，不得永久挂起。

实现形态建议轻量轮询（running/queued 期间开、终态即停，如 5s 间隔）；若施工
时发现更贴近的既有刷新通道（生长 Pill 的更新路径）可复用则优先复用。

## 法证证据（2026-11-10 实码）

- `index.tsx:157-160`：`refreshGenerationStatus` 仅挂载执行一次。
- `useCourseCreation.ts:175-195`：从 generationTasks 快照重建
  `running`/`queued` 态 + 伪 request；此后**没有任何代码路径**能把它写到
  终态——`generateCourse`（唯一写 completed 的地方）的 HTTP 早已不在飞行中；
  WS 帧只进 `CourseGenerationProgress` 的事件列表，不改 status；无轮询。
- 后果实测推演：结构任务 15:01 已 done，若用户中途切页再回，悬浮指示条
  （`index.tsx:446-452`）与弹窗永远「生成中」，取消按钮是唯一出口。
- 数据基础齐备：`GenerationTaskView` 带 `course_id`（`generation_tasks.rs:77`）、
  终态 state（`:80`，快照「在跑/终态一览」不删终态记录）、`step/detail`
  （`:83-85`）。
- v2 知识库流同病同修：v2 in-request 循环在页面切换后继续跑（fetch 不断），
  但旧 hook 实例已随页面卸载——回来后恢复的 running 态同样永不收敛。

## 实施要点

- [ ] `useCourseCreation` 增加恢复态跟踪：恢复成功后启动轮询/订阅，追踪
      active 任务的 `state` 变化；终态映射如 What to build。
- [ ] `done` 收敛：`course_id` → 既有课程详情端点 → `setGeneration(completed)`
      ——卡片「开始学习」直接可用；请求形状补全 `course_kind`（快照无该字段
      时按 task kind 推导，structure/mount/repair 均 graph 流）。
- [ ] 组件卸载/对话框关闭语义不回退：关闭对话框不终止生成（既有契约），
      收敛机制随 hook 存活；页面卸载即清理定时器。
- [ ] `refreshGenerationStatus` 的 `if (generation) return` 守卫保持——收敛
      机制只挂在恢复出的 generation 上，不干扰 in-session 生成
      （in-session 由 HTTP 回归收敛，双写需幂等）。
- [ ] 单测：恢复 running → 快照翻 done → completed + course_id 正确；
      done → failed → failed；任务消失 → cancelled。快照拉取函数可注入假
      时钟/假数据源（参照本目录既有 hook 测试先例
      `useCourseLearning.test.ts`）。

## 验收

- 真机：建课（结构生成中）→ 切走再切回 → 指示条 running → 等 done →
  指示条/弹窗自动翻完成 → 点「开始学习」进入课程。
- 真机或单测覆盖：取消路径（恢复 running → cancelGeneration → cancelled 态
  既有呈现不回归）。
- `bun run typecheck` 绿；`cargo test -p nomifun-learning`（若动了快照端点
  语义，预计不动）。

## 非目标

- WS 终态帧驱动的即时收敛（轮询已够，帧可作后置优化）。
- queued 位次/排队时长的实时精度。
- in-session（未切页）生成路径——已由 HTTP 回归收敛，本票不动。
- 恢复态文案本地化（topic 显示归 c3）。
