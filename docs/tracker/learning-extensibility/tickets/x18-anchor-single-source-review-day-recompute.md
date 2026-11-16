---
id: x18
title: "anchor 单源 + review_day 重算工具——幂等键格式与本地日定义成为可演进契约"
type: task
status: open
assignee:
blocked_by: [x21]
part_of: ../map.md
---

# x18: anchor 单源 + review_day 重算工具

**What to build:** `xp_events.anchor` 的格式/解析收成单源并固化 v1 契约；`review_day` 明确为
派生列并提供重算命令——全仓第一个 backfill 类工具。

## 现状（盘点 2026-11-01，行号实现会话复核）

- `learning_xp_events.anchor` 是 UNIQUE `(user_id, anchor)` 幂等键（081，未发布），格式
  `node:…/review:…/quiz:…` 在 `service/xp.rs:137` 与 `service/quiz.rs:510` **两处各自拼接**；
  格式一变即重复结算或去重失效，且无版本前缀。
- `review_day INTEGER YYYYMMDD`（本地 02:00 滚动、`tz_offset_minutes`）落三处：
  `checkins`（**已发布** 042，UNIQUE `(user_id, review_day)`）、`review_log`（**已发布** 052）、
  anchor 内。行里有 `ts`，理论可重算；但全仓**没有任何** backfill/rebuild 命令，tz/滚动
  规则一变无法回算。
- `nomicore` 已有子命令框架（`learning-smoke`、`doctor`，`nomifun-app/src/commands/`）。

## 实施要点

- [ ] `Anchor` 类型：`enum Anchor { Node{..}, Review{..}, Quiz{..} }` + `fmt`/`parse` 单源；
      两处拼接改调用；格式文档化为 v1——**已裁决不加版本前缀**（2026-11-02 Q16）：
      `node:/review:/quiz:` 的 kind 前缀即判别位，单源 `parse` 拒未知形状，将来格式变就用新 kind
      前缀、不动旧行。081 未发布，种子口径在 x21 基线内定。单测：往返 + 未知形状拒绝。
- [ ] `review_day` 契约：写进 `CONTEXT.md` 复习口径「派生列，源 = ts + tz_offset + 02:00
      滚动；规则变更用重算不用迁移」。
- [ ] 重算命令 `nomicore learning-recompute-review-day --db <path> [--dry-run]`：按当前规则
      重算 review_log/checkins/anchor 内的 review_day；冲突（checkins UNIQUE 碰撞）报告不
      覆盖；dry-run 打印差异计数。`--data-dir` 须放子命令前（既有坑）。
- [ ] smoke `xp` 档加读数：重算 dry-run 差异=0。

## 验收

- `cargo test -p nomifun-learning -p nomifun-app`；`learning-smoke xp` EXIT=0；手工改 tz 后
  重算命令按预期改写（dry-run 先看）。

## 非目标

- 改滚动时刻/时区策略本身。
