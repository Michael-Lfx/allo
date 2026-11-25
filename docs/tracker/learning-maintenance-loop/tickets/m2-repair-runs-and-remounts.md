---
id: m2
title: 让修补真正跑起来——定时臂 bug + 修补后自动重挂
type: task
status: open
assignee:
blocked_by: [m1]
part_of: map.md
---

## Question

修补回合（ADR-0012 / j09）在代码里完整存在，但**三条触发路径全死**：定时臂
有 bug 永不触发、手动按钮从没人点过（动作簿零行）、sb2 的唤醒只挂载不修补。
结果 16 条 findings 从未被任何消费者读过。

## 已核验事实

- **W1**：`nomifun-app-server/src/lib.rs` 的调度循环里，`last_run` 在**每个
  30 秒 tick 之后**被无条件重置，而到期判断要求 `elapsed() >= 间隔`（下限
  30 分钟）——任何运行时长下都不成立。定时维护自上线从未运行。
- 修补输出只有 7 种结构 op（`add_edge / remove_edge / flip_edge / add_block /
  split_block / rename_block / merge_blocks`）；不能新增节点、不能改别名。
- 修补应用 = 结构 hash 变 = **全部挂载失效**（`mounts_stale`）。现状没有
  「修补后重挂」的链路：只能手动点「重新检查」，或靠下一次生长唤醒。
- 水位 = **已读**（applied/noop 都推进）。读过的 findings 不会重复触发——
  定时臂跑起来不会空转燃烧。
- sb2 已有唤醒通道：`take_growth_wakes` / `growth_wake_notified` /
  `spawn_learning_growth_wakes`（仅挂载回合）。

## R

- **R-a 修定时臂**：`last_run` 只在**定时臂真的跑完一轮**时重置（把重置挪进
  「到期并执行」分支）。唤醒分支维持现状（不碰 `last_run`、不刷新节拍）。
  失败/跳过按既有语义（失败也重置，防热循环）。**这是 bug 修复，不是行为
  设计变更**；但它是定时维护第一次真正运行，须走 R-d 的验证顺序。
- **R-b 修补后自动重挂**：`run_structure_repair_with_trigger` 在
  `applied`（**不含 noop**）后，登记一次仅挂载唤醒（复用 sb2 通道）。noop
  不重挂（结构没变，挂载没失效）。失败/取消不登记。
- **R-c 修补输入不变**：仍读「水位之后的 findings + 挂载观察」。信任层已经
  有全量复核与「repair 永不删 historical」两条护栏；不在本票扩它的输入契约。
- **R-d 验证顺序（硬性）**：
  1. 先手动 POST `structure/repair` 跑一次本课（16 条 findings 在场），
     检查产出的 ops 是否只动结构、是否通过全量复核、结构 hash 是否变化；
  2. 若 ops 为空（noop）：接受，说明这 16 条不是结构问题（与 map 的判断
     一致：概念级缺口修补改不了）；
  3. 只有 1/2 都符合预期，才把定时臂的修复留给后续真机自然运行观察。
- **R-e 读数**：修补回合的 `applied/noop/skipped`、op 条数、结构 hash 前后
  值、触发的重挂是否成功——落动作簿与日志（现有通道，不新建）。

## Acceptance

- 单测：
  ① 定时臂到点后跑且 `last_run` 推进；未到点不跑且 `last_run` **不变**
  （W1 的正面钉：把旧行为写成会红的断言）；
  ② 唤醒分支不改变 `last_run`（现有语义回归）；
  ③ 修补 `applied` → 登记一次仅挂载唤醒；`noop` → 不登记；失败 → 不登记。
- `cargo test -p nomifun-learning` 全量绿 + `cargo test -p nomifun-app-server`
  + `cargo check --workspace`。
- **真机**（冻结二进制 + dev 库）：
  1. 手动跑一次修补，记录 ops / hash / 动作簿行；
  2. 若 applied：观察自动重挂是否在数秒内完成、挂载表是否恢复一致；
  3. 若 noop：如实记录，并在 Resolution 写明「这 16 条不是结构问题」。
- 读数进 Resolution。

## 回退条件（负优化判定）

- 定时修补在生产把关过的批次改坏（结构 ops 应用后全量复核仍放过明显错误）
  → 回退 R-a（恢复 `last_run` 每 tick 重置），定时臂退回不运行；m1 不受影响。
- 修补后重挂失败或把新节点挂错块 → 回退 R-b，重挂退回手动。
- 定时臂跑起来后出现「每 6 小时空转烧一次 LLM」→ 检查水位是否被正确推进
  （若 noop 未推进会重复触发），修水位后重试。

## 范围外

- 修补的输入契约扩展（例如把手动「移出本批」的节点回收进结构）。
- 修补 op 的 dry-run 模式（先把 ops 给用户看再应用）——若 R-d 第 1 步发现
  ops 质量不可接受，另立票做 dry-run。
