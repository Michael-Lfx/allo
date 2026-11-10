---
id: c2
title: "恢复态 running/queued 终态收敛——指示条与弹窗自动翻到 done/failed/cancelled，完成可进入课程"
type: task
status: closed
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

## Resolution（2026-11-10 施工收口）

**代码面验收全过，已闭票；真机「切走再切回」与取消路径留待用户实测，FAIL
按票重开。v2 done 分支受后端契约限制未达全票面，记后置（见下）。**

- 施工 = `e733cac5e`：`startRestoredGenerationConvergence` 5s 快照轮询、按任务
  键跟踪、终态即停——done → 快照行 `course_id` 拉课程详情 → completed；
  failed/cancelled 对应翻面；任务从快照消失 → cancelled 兜底；拉取失败不臆造
  终态、下轮再取。恢复请求形状复用 c1 的 `buildRestoredRequest`（无第二套
  推导）；in-session 路径仅加「先停恢复轮询 + 幂等写回」。票面三个场景 +
  取消路径 + 轮询节奏共 15 条单测。
- 评审修复（`10b2cb636`）：
  - **恢复态重试守卫（新可达真 bug）**：票面「cancelled → 既有重试面」前提
    不成立——恢复态能到 failed/cancelled 后「重试」首次对恢复态可达，而
    `retryGeneration` 重发的是合成请求（description = 后端 topic 标签如
    「知识结构生成」），后端会据此**新建一门以该标签为名的学习图课程**。修：
    `retryableGenerationRequest` 恢复态恒 null，终态面板改出本地化指引、
    不出重试按钮；in-session 重试不变。
  - 恢复态 failed 的 error 改由词典出（`learning.restoredTaskFailed`），后端
    中文 detail 不再进弹窗 error 通道，failed ⇒ error 非空。代价（宁缺不
    泄漏）：恢复态失败不再显示具体原因。
  - 恢复的 queued 任务开跑后翻 running（原停「排队中」，票面未要求的规格
    缺口，`onRunning` 可选回调）。
- 与 d7 的合流（本波合并 feat/learn0928 新尖端 `a2f0d288c` 时生效）：d7 把
  恢复过滤从「除 growth 外全部」收窄为只认 `outline`/`structure`（审计实锤
  的存量误判修复），取代票面「kind 过滤沿用 `!== GROWTH_TASK_KIND`」；
  `buildRestoredRequest`/`restoredTaskTopic` 的 mount/repair/lesson 分支经
  恢复路径不可达，保留为防御。
- **已知局限（后置，本波未修）**：v2 outline 任务注册时课程尚不存在，注册表
  `course_id` 段是会话 id（`generate.rs:129-134` 注释明写刻意为之）→ 恢复的
  v2 任务翻 done 后按该 id 拉详情 404 → 落地为 failed + 原因，课程实际已
  建成。「v2 同病同修」在 done 这一支不成立；修复需注册表契约变更（真实课程
  id 字段或中途改键），待在飞恢复波尘埃落定后另立。图流（结构任务登记真实
  课程 id）不受影响。
- 测试缝留痕：仓内无 DOM 测试库，`retryGeneration` 调守卫与 `onRunning` 翻
  状态两处接线只有纯函数测试 + typecheck 兜底。留痕不修（评审 judgement）：
  同快照端点的第三处手写轮询（复用生长 Pill 轮询需改其契约与测试，超票面
  文件面）。
