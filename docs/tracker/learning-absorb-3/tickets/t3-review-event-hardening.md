---
id: t3
title: "复习事件加固定案：推进前快照、同日折叠、幂等键"
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

allo 复习卡已是题级（比 learnhub 旧版先进），但 review 事件缺三件 learnhub #100 的加固，
都是低成本高价值。本票定案：

1. **推进前快照四字段**：`elapsed_days/stability_before/difficulty_before/r_pred` 加进
   复习推进记录（永远落盘、无则 null 不省略）——「事后不可复算：参数重训会污染回放」；
   这是将来校准读数（预测 vs 实际）与 #89 式归因的地基。
2. **同日折叠显式语义**：同卡同学习日重复推进 = Folded（不落新事件）——防重复计卡虚高
   （与二轮 g1「打卡并入 XP」后的日读数正确性直接相关）。
3. **同题幂等键**：建卡 = 一次性事实（重复建卡 fail loud 而非静默重置）。

产出：字段落点（复习事件/复习行/新事件表？——随 allo 现有 review_log 形态定）、折叠
语义的判定口径（review-day 02:00 翻转沿用）、测试口径。参考：learnhub `domain/review.rs`
（`advance_card`/`Folded`/`BeforeSnapshot::initial()`）与 persistence v1.17 事件族原文。
