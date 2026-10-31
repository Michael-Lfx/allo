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

## Resolution（2026-10-31 grilling 裁决：范围收窄）

**裁决（用户：按推荐）**：w2 收窄为三件——**numeric 答案键工具算 + 变式换参数
+ 修复变式 `tol: None` 硬编码缺陷**；「多步题型 + 部分分」**剥离为后置票**
（触发条件：产品要求数学多步题）。等 w1 真机判据落地再动（隔波）。

**影响面核查勘误（2026-10-31 实码核实）：**

- **现状真缺陷**：变式 numeric `tol: None` 硬编码（quiz.rs:819）→ evaluate 按
  1e-9 精确判（progress.rs:482-494，绝对容差）——模型口算误差直接判错学习者。
  w2 换工具算同时修复。
- **「多步题+部分分」无载体**：九题型（models.rs:1464-1481）无多步形态；AI 批改是
  单文本整体评分（open_judge.rs）；settle 部分分通道（settle_review ratio）生产恒收
  0/1（xp.rs:132）；quiz 线纯 objective（选题 SQL 白名单）是有意设计——那是新题型
  设计，非「接核验器」。
- **票面措辞勘误**：settle.rs 不含 tol 比对，四档（0.9/0.6/0.3）是**整场准确率**
  授予门槛而非单题部分分；题号铸的是 **UUIDv7**（LearningActivityId）非 ULID。
- smoke quiz 现只种 single_choice（learning_smoke.rs:1916-1944）——w2 需新增
  numeric 种子题 + grade 断言（2090-2124）才有真机判据落点。
- numeric 全链：生成条款 generation/mod.rs:114（answer JSON number + tol）/:129
  自检；ActivityPack models.rs:1429-1462；落库 config_json（StoredActivityConfig
  3350-3377，serde default 容旧行→**零迁移可扩表达式+参数字段**）；
  LESSON_ACTIVITY_SYSTEM（mod.rs:142-166）**无 numeric 形状条款**，换参契约要新增；
  变式单次 LLM 调用一次给题面+答案，无参数分离（quiz.rs:768-852）。
