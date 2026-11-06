---
id: u08
title: "修补回合：应用模拟+复核并入静态门走反馈重试；快照补主干名单"
type: task
status: open
lane: C
blocked_by: []
part_of: ../map.md
---

# u08：修补的错误要能回到模型手里

## 问题与证据（审计④#1#2，证实；修补无真机档案，影响为推断）
- add_edge 同三元组重复、remove/flip_edge 无命中、应用后 gd.* 全量复核红条，都在 run_repair_round 返回后才暴露，service 直接 Failed 终局（`service/learning_graph.rs` ~:1025-1087），反馈从不回给模型；`validate_ops` 的应用序模拟只查端点/slug（`structure_ops.rs` ~:319-425）。历史病历：修补重复 add_edge 失败。
- 修补快照只有「- 块名（slug）」+ 边行（`service/learning_graph.rs` ~:938-953），提示词要求 add_block 带 spine（`structure-repair.md` ~:30）却不给合法主干名；写错 → 应用后 `gd.blocks.spine_ref` 红 → 终局。

## 实施要点（机械，不挂开关）
- [ ] 回合内校验扩为「静态条款 + **在结构快照副本上试应用 ops + 跑 gd.* 复核**」：边重复、删除/翻转无命中、引用不存在主干、复核红条——全部转为条款违约走既有反馈重试（同指纹早停照旧），只有重试用尽才失败。
- [ ] 快照补一行「主干：slug1、slug2…」（结构真实主干名单）。
- [ ] 新测：重复 add_edge / 无命中 remove / 错 spine 三类现在得到可读反馈并能在下一轮救回；快照含主干行；既有修补测试全绿。

## 三件套
- **主指标**：可一行修正的错误从「终局失败」变为「反馈重试可救」（单测钉三类）。
- **护栏**：合法 ops 的应用结果逐字节不变；noop/skipped 语义不变；不新增终局路径（map 总约束 3）。
- **推翻条件**：无（机械）；若修补回合平均调用数异常上升需复查同指纹早停是否生效。
