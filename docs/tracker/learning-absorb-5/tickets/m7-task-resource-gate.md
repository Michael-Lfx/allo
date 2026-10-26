---
id: m7
title: "学习任务资源闸——并发上限 + 排队 + 取消窗口（#144/#145 缩编版）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# m7: 学习任务资源闸（#144/#145 缩编版）

**What to build:** allo 的学习生成任务目前只有键级互斥、无并发上限——用户连点
多课/多节点会无界并发打 LLM。吸收 learnhub #144/#145（ADR-0029）的**缩编版**：
全局并发上限 + FIFO 排队 + queued 状态可见 + 排队期取消窗口双侧闭合。不移植
完整资源元组注册表（allo 任务面小，全部学习域任务，course 键已天然存在）。

## allo 现状

- `service/generation_tasks.rs`：进程内 `Mutex<HashMap<String, TaskRecord>>`
  :130；注册同键运行中拒绝 Conflict :165-182（「generation task already
  running」）；**无 Semaphore、无队列、无全局上限**。
- 任务派生点：`tokio::spawn` 三处——structure（learning_graph.rs:125）、
  outline（:330）、growth（:1383）；k02 维护 tick 为逐课串行（自带纪律但
  不占注册表并发账）。mount/repair 走 slot 锁（learning-mount-/learning-repair-）
  亦在注册表面。
- `TaskRecord` 状态面：running 终态首写即定（j03）；`GET /generation-tasks`
  + cancel 端点（routes.rs:250-279）；前端有任务读数消费。
- learnhub 事故面 allo 同样暴露：排队取消窗口（半观察到 queued 记录又跑
  body）、panic 容量泄漏（任务 panic 后 cap 永久占用）。

## learnhub 参考（ADR-0029；413ac96/2c3de33/6061222）

- `app/generation.rs`：`MAX_RUNNING_TASKS=5` :97；TaskRegistry
  queued→running→done|failed|cancelled（:209）；内部
  `Sched{queue: VecDeque<Waiter>, held: HashMap<key,Vec<Resource>>, running}`
  :306-314；`run_task` :547-687（提交+排队一临界区 → 等待循环：取消检查 →
  condvar → 出队 + 全量获取 + running 注册单一临界区 :597-665）；
  **body 套 `catch_unwind`**：panic 记 failed 再 resume_unwind——容量永不泄漏
  （:672-684）。
- 资源元组（缩编版不移植，但语义参考）：Grow=[Course, Registry]（跨课生长在
  Registry 上串行——别名读改写 + 铸名近邻裁决的进程级安全）；零资源任务仍占
  5-cap（task_resources.rs:71/:88-92）。
- 排队取消窗口双侧闭合（6061222）：取消与出队同一临界区仲裁——半观察到的
  queued 记录不可能跑 body（:467-471 注释，测试
  `cancel_during_submission_never_lets_queued_body_run`）；queued 取消路径
  :600-618（出队 + cancelled 终态，body 零执行）。
- 进程级模型调用上限另有一层：`MAX_CONCURRENT_MODEL_CALLS=5`
  （infra/concurrency.rs:30，ThrottledExecutor 包装单源）——两层各管各的
  （任务面 vs 调用面）。
- 前端 queued 渲染（QUEUED_NOTICE 单源）。

## 实施要点

- [ ] **缩编设计（票面推荐，实施可复议）**：`generation_tasks.rs` 内嵌
      `tokio::sync::Semaphore(5)`（FIFO 公平）+ TaskRecord 增 `queued` 状态；
      提交 = 先注册 queued → acquire → 置 running 跑 body；**排队取消窗口**：
      cancel 对 queued 记录 = 释放排队位 + 终态 cancelled（body 零执行）——
      用「acquire 前再查终态」+ 取消与状态迁移同锁（现有 Mutex 即临界区）
      实现双侧闭合。
- [ ] **panic 容量防漏**：body 包 `catch_unwind`（或 tokio 任务 + guard drop
      释放 permit 的 RAII——二选一，实施定），panic 记 failed 终态再上抛。
- [ ] 资源语义缩编：全局 cap 5 + 既有键级互斥 = 两层；**course 级是否再串行**
      票面裁决点（learnhub Grow 持 Registry 跨课串行的理由是别名读改写/铸名
      裁决的进程级安全——allo 的 register_alias_tx 在 apply 事务内、defnear
      纯读，风险面不同；推荐先不串行，冲突实测再说）。
- [ ] `GET /generation-tasks` 增 queued 可见（TaskRecord.queued_at/position）；
      前端任务面 queued 渲染 + i18n。
- [ ] 维护 tick 与本闸的关系：k02 逐课串行是 tick 内纪律；tick 提交的
      mount/repair 任务同样过闸（占并发账）——票内注记，实施核实。
- [ ] 开关：`NOMI_LEARNING_TASK_GATE=off`（off = 无上限直跑 = 现行为）。
      上限数值常量化（learnhub 5，缺省同值）。

## 验收

- **单测（红绿）**：并发上限（6 任务同时提交，5 跑 1 排队）；排队取消（queued
  期 cancel → 终态 cancelled、body 零执行——learnhub 同名测试移植）；运行中
  取消路径不回归；panic 泄漏防（body panic → permit 释放 + failed 终态，后续
  任务可跑满 5）；键级互斥不回归；off 臂无上限。
- **smoke 双臂（开关臂）**：maintenance 档或新链路——并发读数行
  （running_max/queued_peak）在 smoke 输出在场；off 臂读数为直跑形态。
  硬判据两臂既有判据不劣化。
- **对照卡**：制造 >5 并发提交的实测（临时脚本或 smoke 夹具），两臂读数并排
  （on 臂 running_max=5 恒成立、queued_peak>0；off 臂 running_max>5）——
  落 Resolution。
- `cargo test -p nomifun-learning` 全绿；typecheck/i18n。

## 非目标

- 完整资源元组注册表（CourseList/Ledger 等资源类型、acquire-all-before-run
  多资源事务）——allo 任务面用不上，缩编记录理由。
- 前端富进度/失焦通知（#118/#119，D 组不吸收）。
- 进程级模型调用上限（learnhub infra/concurrency.rs 那层）——allo completer
  层无此缝；若 m6/m8 需要再议。

## 备注

learnhub #145 迁移教训：**会话内层零资源防嵌套死锁**（外层持 Registry 再进
内层会自锁）——allo 缩编版只有一层 permit，天然无此坑，但 m6 若把补批循环
放进闸内，循环里再提交任务就是同形态嵌套——票内已注记依赖方向。
