---
id: b1
title: growth_running 按课程+kind 收窄（跨课程串扰）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

任何课程跑任何生成任务，所有课程的知识结构卡都标「生长中…」+ 补充节点
钮禁用/加载。本课无任务也被点亮。作用域如何收窄？

## 已核验事实（2026-11-17 对码，证据链见 map.md）

- 根因：`generation_running()`（learning_graph.rs:4972-4977）对
  `generation_tasks` 快照做 `any(RUNNING)`——课程与 kind 双无过滤；
  `:4058` 注入每课 graph 视图的 `growth_running` 字段（models.rs:2367）。
- 七类任务任一在跑都命中（含别课的 STRUCTURE/LESSON）。
- 前端消费三处：徽章（:714 生长中…）、次要行（:690）、补充节点钮
  loading+disabled（:871/:876，注释「growth_running 是服务端真相 d7」
  ——服务端真相本身是错的口径）。
- 前端其它源（WS :322 / hasActiveStructureRow）已正确过滤，无需动。

## Terms（方案草案，施工前过裁决）

- **R-a（主修）**：graph 视图的 `growth_running` 改为本课本 kind：快照行
  过滤 `task.course_id == 该课 && task.kind == KIND_GROWTH && state ==
  RUNNING`（快照行自带 course_id——`begin_generation_task(kind, course_id,
  …)` 注册时已写，零 schema 变更）。函数签名改为 `generation_running(
  course_id)` 或在 :4058 处内联过滤（择测试面小者）。
- **R-b（同族排查）**：确认 `generation_running()` 无其它调用点（已核：
  全仓唯一 = :4058）；顺带核查 auto refill 的补批是否注册为 KIND_GROWTH
  （是则自动臂天然被同一过滤覆盖；否则补）。
- **R-c（回归测试）**：后端测试两门课——A 课跑 KIND_GROWTH，B 课 graph
  视图 `growth_running=false`；A 自己 true；A 任务终态后 B/A 都 false。
  前端消费面零改动（字段语义修准，不是 UI 改动）。
- **R-d（不做）**：structureRunning 徽章面不动（已正确）；G5/D7 防呆
  （结构生成期间显式生长放行）依赖的是前端本课 structureRunning，不受
  影响，不动。

## Acceptance

- 后端测试：跨课程隔离 + 同课 kind 隔离（如 LESSON 任务在跑不算
  growth_running）+ 终态消解，全绿。
- 真机复验路径：建新课跑结构期间，旧课详情页徽章不亮、按钮可点
  （随波真机欠账走查）。

## Resolution

已施工收口（2026-11-17，feat/learn1116：`42a77a47a`）。

- `generation_running()` 更名 `growth_running(course_id)`（方法名随语义
  更名，全仓唯一调用点同步）——快照过滤 `state == RUNNING && course_id
  == 本课 && kind == KIND_GROWTH`；零 schema 变更（快照行自带
  course_id，resolved_course_id 优先取法核实过 outline 键段口径）。
- auto 补批与手动生长同走 begin_growth_task → KIND_GROWTH（:4993），两
  臂天然覆盖；同族排查干净（其余 snapshot 消费均为正当全局面）。
- 回归三隔离：跨课（A growth RUNNING → B false）/ kind（B LESSON
  RUNNING → A true 不受扰）/ 终态消解，全绿（nomifun-learning 970→975
  绿经审查修复批）。
- 真机复验欠账：建新课跑结构期间旧课徽章不亮、按钮可点。
