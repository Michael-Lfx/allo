---
id: sb2
title: 生长完成唤醒维护回合（仅挂载）——地形清单折叠的数据源时效（ADR-0018 决定 4）
type: task
status: open
assignee:
blocked_by: [sb1]
part_of: map.md
---

## Question

地形清单（sb1）以挂载为「已覆盖」的唯一依据，而挂载表全库 0 行：手动入口
从未被触发；维护回合的定时臂有缺陷、从不触发。没有挂载数据，地形清单永远
等于全量，sb1 的教练侧瘦身不生效。

## 已核验事实（2026-11-24，读码；行号对应尖端 5c7be0425）

完整笔记 `C:/Users/test/Desktop/work/allo-sb-notes/sb-facts.md`（#13–#21）。

- **定时臂是 bug，不是慢**：`nomifun-app-server/src/lib.rs` 调度循环里
  `last_run` 在 `:2873`（loop 体内、内层 async 块外）**每 30 秒被无条件重
  置**；到期判断 `:2833-2835` 要求 `elapsed() >= due`（下限 30 分钟）——
  永不成立。`:2833` 的 `return` 只退出内层 async 块。定时维护自上线从未
  运行。**本票不修它**（Q31 a：修了会激活从未在生产跑过的定时修补臂）。
- **循环无任何外部唤醒通道**，只有 `tokio::time::sleep(30s)`。tokio 是
  workspace 级 `features = ["full"]`；仓内「sleep 或被叫醒」的参考写法在
  `nomifun-agent-execution/src/event_publisher.rs:55-82`（`Arc<Notify>`）。
  调度器在 `nomifun-app-server`，生长跑在 `nomifun-learning` 的
  `LearningService::growth_body`——唤醒通道端点只能挂在 `Arc<LearningService>`
  上。
- **`run_maintenance_tick`**（service:1726-1834）：扫全库 enabled 图课程、
  逐课 `maintenance_needs` → `needs_repair` 先修补 → `needs_mount` 再挂载；
  trigger 硬编码 `Timer`（:1797、:1819），无参数可传别的触发源。只带一个
  `user_id`。
- **「从未挂载」≡ `mounts_stale = true`**（service:1621 `None => (Vec::new(),
  true)`），此时 `pending_mounts = 全部节点`、`needs_repair = true`。读侧
  `structure_view`（:4232-4238）对「无行」取 `stale = false`，与维护侧
  口径相反。
- **findings 不是惰性日志**：`maintenance_needs`（:1565-1573）统计水位之后
  **全部** findings（不分 source），`needs_repair = unconsumed_findings > 0 ||
  mounts_stale`（:1639）；修补回合把它们渲染进提示词（:1036-1061）。
- **`SystemActionTrigger`**（`system_action.rs:39-49`）无 serde，落库走
  `as_str()`，DB 无 CHECK → 新增变体**不需要迁移**。唯一的穷举 match 是
  `as_str`（:53-58）。`records_actor` 用 `matches!`，新变体自动 `false`。
  前端映射表 `ui/src/renderer/pages/learning/systemActions.ts:13-19`，未命
  中落 `unknown`（显示「自动」）。
- **生长批落库的收尾点**：`growth_body` :3059 `apply_batch` 提交事务之后、
  :3061 `Ok(applied)` 之前；作用域内有 `course_id`、`engine.trigger`、
  `engine.user_id`、`applied.idle`。空手批分支（:3037-3050）不经过这里。
  生长持槽 `graph-growth-{course_id}`，挂载持槽 `learning-mount-{course_id}`
  ——互不阻塞。
- **`LearningService` 是 `#[derive(Clone)]`**，进程内状态都是 `Arc<…>` 字
  段；`new` 逐字段显式初始化（无 `..Default`），加字段要同步加初始化。
- **任务闸**：`GenerationTaskRegistry` 的全局并发闸（上限 5）过闸是
  `.await` 等待——唤醒路径若在调度循环体里内联 `await` 挂载回合，满闸时
  会卡住调度循环。
- **`start_learning_auto_maintenance` 零测试覆盖**（app-server 的三个测试
  都不测真实循环；该 crate 无 `serial_test` 依赖）。把唤醒的**判定与登记**
  放在 `nomifun-learning`（service 层）才可测。

## R（ADR-0018 决定 4；用户裁决 2026-11-24，含 Q31/Q32）

- **R-a 唤醒而非内联**：生长批落库后（`growth_body` :3059 之后、非空手批、
  `trigger` ∈ {Settlement, AutoRefill}）调用 service 的「登记唤醒意图」；
  **不**在生长的后台任务里调用挂载。调度循环的 `sleep(30s)` 改为「sleep 或
  被唤醒」（`tokio::select!` + `Notify`，端点挂 `Arc<LearningService>`）。
- **R-b 仅挂载模式**：`run_maintenance_tick` 增运行模式参数（或新增仅挂载
  的入口函数——取改动面小者）；唤醒路径只对**被登记的课程**走挂载分支，
  跳过修补。定时路径行为逐字节不变（含 W1 的 bug——不修）。
- **R-c 唤醒准入**（判定放 service 层纯函数，可单测）：
  - `!mounts_stale`（整行失效/从未挂载需全量重挂 → 不自动）；
  - 待挂节点数 ≤ 8（个别节点 slug 失效计入待挂数，属增量，照常）；
  - 去抖：距该课上次**成功**挂载 ≥ 5 分钟；同课一次生长只登记一次。
- **R-d 不唤醒只写日志（Q32）**：条件不满足时 `tracing::warn!`（带课程
  ID 与原因：`stale` / `pending_over_threshold` / `debounced`），**不写
  findings**（findings 会触发并喂入修补回合）。
- **R-e 失败只留痕**：挂载失败走既有 Failed 动作簿行；不回滚已落库的生长
  批、不重试整链；**不额外写 findings**。节点仍在待挂集，下次唤醒再试。
- **R-f 触发源**：`SystemActionTrigger` 新增 `GrowthFollowup`
  （`as_str` = `"growth_followup"`）。同步：`system_action.rs` 的两个测试
  （`machine_names_are_the_wire_contract` 加一行、`only_manual_records_the_
  actor` 的数组加一项）；前端 `systemActions.ts` 映射键 + zh-CN / en-US 两
  条词典（「随生长附带」/ "Following growth"）+ `bun run gen:i18n` +
  `bun run check:i18n`。
- **R-g 身份**：唤醒意图携带 `engine.user_id`（生长后台任务里现成），
  挂载回合以它运行；不用 `installation_owner_id` 再解析。
- **R-h 调度循环不被阻塞**：被唤醒后，调度循环对登记的课程 `spawn` 一个
  仅挂载任务（或在循环体内运行但不持任何会卡住的锁）——避免任务闸满时卡
  住 30 秒 tick。`LearningMaintenanceLivenessGuard` 与 panic 兜底语义不变。

## Acceptance

- 单测（`nomifun-learning`，Scripted completer，全部 `#[serial]`）：
  ① 预置**非 stale 的挂载行**（参考 `structure_mount_round_mounts_then_skips`
  :11080 的做法——先跑一次真实 `run_structure_mount`）+ 长出一个新节点 →
  自动触发生长落库 → 登记唤醒 → 以仅挂载模式挂上新节点、账本行
  `trigger_source = growth_followup`；
  ② 仅挂载模式下有未消费 findings 的课**不跑修补**、结构不变；
  ③ `mounts_stale = true`（含「从未挂载」的默认夹具）→ 不登记、零挂载调用、
  一条 warn；
  ④ 待挂 9 → 不登记；待挂 8 → 登记；
  ⑤ 5 分钟内二次生长 → 去抖不登记（时钟经注入的 `now` 函数或
  `Instant` 参数化，不真实 sleep）；
  ⑥ 挂载失败 → 生长批仍在库、Failed 动作簿行在、**findings 表零新增行**；
  ⑦ 手动触发生长不登记唤醒；
  ⑧ `SystemActionTrigger::GrowthFollowup` 的机器名与 `records_actor ==
  false`。
- `cargo test -p nomifun-learning` 全量绿（基线以 sb1 合并后为准，只增不减）
  + `cargo check --workspace` + fmt。
- 前端：`bun run typecheck` + `bun run check:i18n`。
- app-server：调度循环的唤醒分支至少有一条测试（把循环体抽成可注入形状，
  或在 `learning_maintenance_tick_guarded` 包裹层复刻——沿既有做法）。
- **真机**：先手动触发一次「结构对齐」把存量节点挂上（预期一次较大调用），
  之后跑一轮自动补批 → 观察唤醒发生、挂载成功、下一轮教练输入的地形清单
  实际折叠比例（展开行数 / 总块数）。
- 读数进 Resolution；折叠比例同步回 ADR-0018 的验证口径。

## 回退条件（负优化判定）

- 唤醒使维护回合在自动补批后稳定失败或明显拖慢（下一轮生长被 busy 跳过）
  → 回退整票；sb1 不受影响（地形清单退回全量）。
- 挂载把新节点挂错块导致地形清单错误折叠、教练因此跳过本应长的地形 →
  提高 R-c 阈值或改折叠口径，换法重立。
- 唤醒后动作簿出现预期之外的修补行（仅挂载模式被绕过）→ 实现有误返工。

## 范围外（另票候选，不在本票）

- **维护定时臂的 `last_run` 缺陷**（W1）：修它会激活从未在生产运行过的定
  时修补臂（改结构、使全部挂载失效），须单独验证。
- 「结构对齐」入口的可发现性。
- 读侧 `structure_view` 与维护侧对「无挂载行」的 stale 口径相反。
