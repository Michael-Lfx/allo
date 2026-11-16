---
id: r2
title: "learnhub 会话与账本体系深挖（#87 线）"
type: research
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub 的题级卡下沉（#100/ADR-0022）、XP 三线入账（#88）、会话体系（#105/#106/#107）、
渲染管线（#108）、xp_calendar（#113）各是什么；对 allo（题级卡已有、XP 二轮已裁未实施、
无会话体系）什么是增量、什么已有等价物？

## Resolution

**速览**：**allo 的题级卡比 learnhub 旧版更早更先进**（ADR-0022 明引 allo 迁移 039 向
allo 对齐）；真正的增量是四类——XP 结算参考实现（二轮 g1 的现成落地件）、复习事件加固、
会话体系（大粒度新产品面）、渲染管线。

- **XP 三线结算（#88）**：教新 = est 分钟；复习 = 工作量当量（题型当量选择 1/短答 2/
  开放 3 分钟 × 完成题数，下限 3）；quiz = 同当量、准确率分级恒非负。分级授予阶梯
  （全对 +bonus 2 / 部分按比例向下取整 / 砍短 0 / 乱猜负分：≥4 题且实际率 < 0.5×基线率
  → −基数÷2）。**写侧条款 validate_settlement**（「数值由调用方算好，落账不计算」+
  形态违约写侧拦截 + quiz 恒非负专属条款）——防脏账关键机制。两把尺（进度 = 结算数、
  XP = 努力账）同源回放、互不换算；每日目标 40 住设置；ETA = 剩余 est ÷ 目标绝不回写。
- **复习事件加固（allo 缺口）**：推进前快照四字段（elapsed/stability_before/
  difficulty_before/r_pred，永远落盘 null 不省略——参数重训会污染回放，事后不可复算）；
  同日折叠显式语义（Folded 不落新事件）；同题幂等（建卡一次性事实 fail loud）。
- **会话体系（#105/#106/#107）**：SessionRecord 落盘（首写为终/不携判分底/abandoned
  「有头无尾也是数据」）；五命令（start_session/swap_window/grade_answer 不落账/
  complete_session 幂等步进/abandon_session）+ 脱敏投影（字段级缺席判分底）+
  统一三型会话页（probe/content/questions/结算卡 + 5 秒回忆门 + 提前收杆）。
- **渲染管线（#108）**：markdown+KaTeX + 声明式可视化分层 + **RenderBoundary fail-visible
  （不静默丢不 crash）** + 「模型产物非权威」徽标 + 手写表达式求值器（绝不 eval）。
- **xp_calendar（#113）**：近 12 周恒 84 桶含零桶（「零与缺席是两回事」）、单趟回放零新
  状态——对接二轮 g1「打卡并入 XP」的日历换数据源即可。
- **allo 已有等价物**：题级卡本体（每题自带 FSRS 卡）、跨课程扁平队列、到期清单、九题型
  （learnhub 三题型的超集）、日历壳（换 XP 数据源）、自建题即卡。

完整报告（含卡模型、事件族原文、定价表、SessionRecord schema、五命令语义、渲染分层）：
[assets/r2-sessions-ledger.md](../assets/r2-sessions-ledger.md)
