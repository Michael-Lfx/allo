---
id: p2
title: "structure 未知顶层键删键放行+findings 留痕——不再整份两产物重生成"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# p2: structure 未知顶层键宽容放行

**What to build:** gd.keys 类「白名单外未知顶层键」违规 → 删键放行 +
findings 留痕（与校验器留痕自述「白名单之外的键一律删除」对齐——现状
自述删键、行为整份拒绝，自相矛盾）；缺键/类型错等硬伤仍走原
`Rejected → regenerating both artifacts` 路径；同指纹早停（e1 D2）语义不变。

**验收：**
- 单测钉住两形态：未知键放行且产物无该键+findings 有行；缺键仍拒
- `cargo test -p nomifun-learning` 基线

**病历依据：** 2026-11-09 attempt1 的 structure 流跑 107.6s 后因模型多吐
一个顶层键 `type` 被拒 → terrain 11.6s + structure 111s 白烧，共 122.7s
（structure 任务 48% 浪费的全部来源）。

## Resolution

已关（2026-11-10）：commit `166ff2334`，并回 feat/learn0928。未知顶层键门前删键放行+warn 留痕，缺键仍 red；旧 gd_keys 测试拆分；同指纹早停保持绿。修复轮补生成级留痕断言（`627049614`，tracing 捕获红绿）。
