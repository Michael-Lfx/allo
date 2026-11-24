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
从未被触发；自动路径（维护回合，默认间隔 360 分钟）在进程启动时「先等一个
缺省间隔」且计时归零——应用连续运行不满 6 小时，维护回合一次不跑。没有挂
载数据，地形清单永远等于全量，sb1 的教练侧瘦身不生效。

## 已核验事实（2026-11-24）

- 调度：`nomifun-app-server/src/lib.rs` `start_learning_auto_maintenance`——
  30 秒 `sleep` 循环，无外部唤醒通道；`last_run = Instant::now()` 随进程
  启动初始化（有意设计，注释「首轮不立即跑」）。
- 回合：`run_maintenance_tick`（service/learning_graph.rs:1731）逐课
  `maintenance_needs` → `needs_repair` 先修补 → `needs_mount` 再挂载；
  `needs_repair` 与 `needs_mount` 已分别给出。
- 待挂集 `mounting::pending_nodes`：未评估 ∪ **失效 slug** 的节点；整行
  `mounts_stale` 时调用方传全量。
- 挂载失败：整回合重试 ≤ 3、不降级，落 Failed 终态行（:833）；取消零行。
- CONTEXT.md「维护回合」：ADR-0012 明文「不内联生长批」——本票保留。

## R（ADR-0018 决定 4；用户裁决 2026-11-24）

- **R-a 唤醒而非内联**：生长批落库后登记唤醒意图（同课去抖键），**不**在
  生长的后台任务里调用挂载。调度循环增唤醒通道（替换纯 sleep），被唤醒时
  对该课跑维护回合。
- **R-b 仅挂载模式**：`run_maintenance_tick` 增运行模式参数；唤醒路径只走
  挂载分支，跳过修补——修补改结构是全局动作，仍只按定时节奏跑。
- **R-c 唤醒准入**（不满足 = 不唤醒，留一行 findings 提示手动「结构对齐」）：
  - `!mounts_stale`（整行失效需全量重挂 → 手动）；
  - 待挂节点数 ≤ 8（从未挂载而节点已多的课，首次唤醒不附带大调用）；
  - 去抖：距该课上次**成功**挂载 ≥ 5 分钟；同课一次生长只登记一次。
  - 个别节点 slug 失效属增量，计入待挂数、由唤醒处理（不受 R-c 第一条限制）。
- **R-d 失败只留痕**：挂载失败走既有 Failed 动作簿行 + 一条 findings；不回
  滚已落库的生长批、不重试整链；节点仍在待挂集，下次唤醒再试。
- **R-e 触发源**：动作簿新增 `growth_followup`（随生长附带）——不复用生长
  的触发源值（动作簿须能看出挂载是附带的）。加法式枚举扩展，旧值不动。

## Acceptance

- 单测（Scripted completer，全部 `#[serial]`）：
  ① 生长落库 → 该课登记唤醒 → 维护回合以仅挂载模式跑、挂载新节点；
  ② 仅挂载模式下 `needs_repair=true` 的课不跑修补；
  ③ `mounts_stale=true` → 不唤醒 + findings 留痕；
  ④ 待挂 9 个 → 不唤醒 + findings 留痕；待挂 8 个 → 唤醒；
  ⑤ 5 分钟内二次生长 → 去抖不唤醒；
  ⑥ 挂载失败 → 生长批仍在库、Failed 动作簿行在、findings 在；
  ⑦ 手动触发生长不登记唤醒（用户在场，维持现状）；
  ⑧ 动作簿 `trigger_source = growth_followup` 可读，`records_actor` = false。
- `cargo test -p nomifun-learning` 全量绿 + `cargo check --workspace` + fmt。
- **真机**：先手动触发一次「结构对齐」把存量节点挂上（预期一次较大调用），
  之后跑一轮自动补批 → 观察唤醒发生、挂载成功、下一轮教练输入的地形清单
  实际折叠比例（展开行数 / 总块数）。
- 读数进 Resolution；折叠比例同步回 ADR-0018 的验证口径。

## 回退条件（负优化判定）

- 唤醒使维护回合在自动补批后稳定失败或明显拖慢（下一轮生长被 busy 跳过）
  → 回退整票；sb1 不受影响（地形清单退回全量）。
- 挂载把新节点挂错块导致地形清单错误折叠、教练因此跳过本应长的地形 →
  提高 R-c 阈值或改折叠口径，换法重立。

## 范围外（另票候选，不在本票）

- 维护回合调度计时随进程启动归零（影响修补的时效；「滞后无害」对修补仍
  成立）。
- 「结构对齐」入口的可发现性。
