---
id: t3
title: "复习事件加固定案：推进前快照、同日折叠、幂等键"
type: task
status: closed
assignee: zhangyj
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

## Resolution

**速览**：三项核实后定案——快照四字段补列；**同日折叠 allo 已有语义，确认对齐不加码**；
建卡幂等显式化。

1. **推进前快照四字段**：`review_log`（050 迁移的复习日志）加四列 `elapsed_days/
   stability_before/difficulty_before/r_pred`（全 nullable；**推进前的参数快照，永远
   落盘 null 不省略**——「事后不可复算：参数重训会污染回放」）；写侧在推进时从卡片当前
   状态铸值；这是将来的校准读数（预测 vs 实际）与归因地基。
2. **同日折叠：allo 已有，确认对齐**。CONTEXT.md Review Queue 词条既载「当日已推进过的
   卡在重新到期前，重复作答（过期界面、并发提交）只记作答流水，不推进、不落日志」——
   与 learnhub `Folded`（不落新事件）同构且更早。本票仅补两件事：①作答流水行补
   `session/elapsed_ms/response` 锚（g4 数据面字段，同批迁移）；②补一条折叠语义测试
   （同卡同学习日二次推进 → 无新日志行、无调度变化）钉死行为。
3. **建卡幂等显式化**：种卡（课时完成 → 自建题建卡）路径加显式存在性检查——已存在 =
   跳过（幂等）而非静默重置；异常冲突（同题已有活卡且状态非预期）= fail loud。allo 的
   课时完成状态机已挡住大部分重复，此处补防御性断言 + 测试。

测试口径：快照四字段推进后逐字段断言；折叠测试（上）；重复种卡幂等测试。

