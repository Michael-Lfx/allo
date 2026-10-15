---
id: g4
title: "会话体系取舍：allo 要不要引入学习会话？"
type: grilling
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub #105/#106/#107 建了完整的会话体系：SessionRecord 落盘（首写为终/不携判分底/
abandoned「有头无尾也是数据」）、五命令（start/swap_window/grade_answer 不落账/
complete 幂等步进/abandon）+ 脱敏投影（字段级缺席判分底）+ 统一三型会话页（逐节步进/
5 秒回忆门/提前收杆/结算卡）。

allo 的学习单位是**课时**（LessonStatus: InProgress/Completed/Skipped 二态 + 进度行），
作答走 activities/attempts，判分本就在服务端；没有「会话」这个粒度——一次学习行为的
起止、逐题作答序列、流失（abandon）信号都无记录。

需要裁决（这是产品形态级决策，不是移植细节）：

- **(a) 引入会话层**：SessionRecord 模型 + 最小命令面（start/complete/abandon 三件起步，
  swap/grade 后置）——获得：逐题作答历史（#110 作答历史抽屉的底座）、流失信号、
  「首写为终」的诚实数据、结算卡读数；代价：新表新命令 + 学习页改造（课时学习器包一层
  会话壳）。
- **(b) 只采纳数据面**：给现有作答路径补「会话 id 锚」+ 作答事件落盘（practice.answer
  的 elapsed_ms/response/session 字段形态），不建会话命令面与统一页——数据先攒着，
  产品面后置。
- **(c) 不采纳**：课时二态够用，会话体系整体后置（含复习中心聚合读数/检测记录读面——
  r3 报告标注这两项依赖会话模型）。

连带面：复习阶梯（#91/#92 R1/R2）、quiz 触发（#90 K 计数）、护栏债务刹车都建立在
「会话/作答事件流」上——选 (c) 则这些一并后置；选 (a)/(b) 则 #89 归因与 #90 quiz 的
「 allo 版」获得地基（是否做仍是独立决策）。

约束：allo 已有的题级卡/复习队列不动；adopt 时「判分底服务端持有」红线 allo 天然满足；
学习页改造须过主题/i18n 红线。
