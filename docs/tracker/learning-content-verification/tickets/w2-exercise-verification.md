---
id: w2
title: "练习题型接核验器——numeric 答案键工具算/变式换参/多步批改"
type: task
status: open
assignee:
blocked_by: [w1]
part_of: ../map.md
---

# w2: 练习题型接核验器

**What to build:** quiz/练习线的可计算量全部改由 verify_math 产出：numeric 题
答案键工具算（tol 只承担数值噪声语义，不再背书模型口算）；q3 变式生成升级为
「换参数 + 工具算新答案键」；多步题 AI 批改对作答中间量逐个核验（部分分依据）。

## 用户动机（2026-10-31 拍板原话要点）

核验基建是之后升级练习题型的地基——本票就是那个「升级」本体：
- numeric 题答案键今天靠模型报数（无确定性背书）；
- q3 变式是「同型换皮」，参数与答案键同为模型生成；
- AI 批改轨对多步作答没有逐步核验的抓手。

## allo 现状（落点提示，实现会话复核）

- quiz 线：`settle.rs` 四档判分（numeric tol 判分、0.9/0.6/0.3 门槛常量集中）；
  q2 回合载体（`learning_quiz_rounds`，plan_json 判底）；q3 变式生成（复用
  generate_lesson_activity 课时加题引擎 + 防复用池 + ArchivingCompleter）。
- verify_math 求值器落 nomifun-learning（w1 已定）——settle/变式同 crate，
  复用最近。

## 实施要点

- [ ] numeric 题生成侧：模型只给参数化表达式与参数值，答案键必须经 verify_math
      eval 实算落库；题面渲染与答案键同源（同一表达式两处消费）。
- [ ] 变式生成：参数化模板换参 → 新答案键工具算；防复用池语义保持（题面变化
      但同模板的判定不变）；变式不落 learning_activities 的既有裁决不变。
- [ ] 多步题 AI 批改轨：作答中间量逐 claim 核验，部分分 = 已核验通过步骤的
      权重；判分反馈带核验细节（哪步对哪步错）。
- [ ] 红绿样：答案键与工具输出一致的断言（含舍入路径）；变式换参后答案键变化
      且精确；批改部分分的边界。

## 验收

- `cargo test -p nomifun-learning` 全绿；fmt；零迁移（如需存表达式+参数则
  另评估，倾向复用现有 JSON 列）。
- 真机判据：quiz smoke numeric 题答案键与工具输出一致；变式题换参后答案精确
  且判分正确。

## 非目标

- 选择/判断/匹配等非计算题型（无 oracle 需求，现状已够）；
- 主观题语义判分（AI 批改轨的 LLM 判断部分不动，本票只加确定性核验层）。
