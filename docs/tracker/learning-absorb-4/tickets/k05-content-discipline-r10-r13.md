---
id: k05
title: "内容纪律 R10 具象替身 / R11 受众语域 / R12 表述工具在场 / R13 断言-例子一致"
type: task
status: closed
assignee: zhangyj
blocked_by: [k03]
part_of: ../map.md
---

# k05: 内容纪律 R10–R13 织入两管线

**What to build:** j07 吸收了 R1/R4/R6/R7/R8（+R9 适配），learnhub ldata1 战役
又铸了四条同族写作纪律。全采（域中立措辞、零新机械门——learnhub 同款发现面
= 写作面条款 + 走查比对）：R10 抽象对象首讲给具象替身、R11 例子情境取受众
生活域、R12 正式表述的记号术语须在已知集内、R13 断言所配例子的数字必须能
实际推出该断言。

## allo 现状

- 纪律块：`generation/lesson.rs:758-768` `CONTENT_DISCIPLINE_BLOCK`（传统
  管线节大纲 + 三风格节正文都插 :834/:854/:869）+ R9 `NARRATIVE_VOICE_BLOCK`
  :769-772；图管线 = agent 系统提示词 `GENERATE_LESSON_AGENT_SYSTEM`
  （`nomifun-ai-agent/src/lesson_content_loop.rs:63-98`）——j07「两管线」口径。
- 概念面只进图路径（j07 `GraphConceptFaces` 首现闭包/引用名/别名池，
  `lesson_draft.rs:955-1036` 观察条先例）。
- 受众数据源：**allo 无课程宪章/audience 字段**；图路径注入程度声明
  （`lesson_user_text` graph 分支 :517 `graph.degree_statement`）与学习目标。
- j07 审计计数器：naming_mixed / naming_canonical_only / first_seen_unintroduced
  （图路径）+ narrative_self_reference / narrative_structure_words（两管线）。

## learnhub 参考（#126 = R10；#148 = R11/R12/R13，契约 v2.8→v2.9 §8 表）

`src-tauri/prompts/contentgen/content-write.md:42-48`「成色要求」节（写作面）：

- **R10**（:42）：「抽象对象（结构、记号、运算）首讲时，先给一个具象替身——
  几何图像、动作隐喻、或具体情境里摸得到的量——让随后的规则与性质能被替身
  解释；关键规则配一句『为什么如此』的语义说明（借替身讲），不让学习者靠
  裸符号死记。」规划面仅取向注记（content-plan.md :5）。
- **R11**（:46）：「例子与情境取课程受众的生活域（受众行声明的人群画像）——
  面向成人的课程，情境人物与场景写成人生活，不幼态、不写低龄课堂情境；讲法
  保留实物与教具，情境不锁龄。」规划面情境取向句（content-plan.md :32）。
- **R12 = 表述工具在场**（:47）：「规则的正式表述所用记号与术语，须在已知集
  内（teaches ∪ assumes 声明的，或本点正文已先行正式引入的）——不在已知集
  的记号与术语不即兴使用，换自然语言表述，或在正文内先正式引入再用。」
  learnhub 明确**零机械门**，发现面 = 写作面条款 + 回放比对 + 真机走查。
- **R13**（:48）：「后果类断言所配例子的数字必须能实际推出该断言——漏数例
  报数应少于总数、重复例应多于总数；操作型演示每一步的中间结果逐一验算，
  只核对终态不算验算。」写作取向，零机械门。

## 实施要点

- [ ] **R10/R12/R13 进两管线写作面**：`CONTENT_DISCIPLINE_BLOCK` 扩三条
      （措辞按 allo 现有块风格微调，语义逐句保持）；图管线
      `GENERATE_LESSON_AGENT_SYSTEM` 成色段同织。
- [ ] **R11 受众语域进两管线**：allo 无 audience 字段——条款措辞锚定既有
      输入面（「课程声明面向成人/职场等非低龄受众时，情境人物与场景写该受众
      生活域，不幼态；讲法保留实物与教具，情境不锁龄」），程度声明/目标文本
      已在两管线注入面内；**不加新向导字段、不改 DB**（受众输入面是产品
      裁决，B 组范围）。传统管线节大纲（`section_outline_system`）加情境
      取向句（learnhub 规划面 :32 同款）。
- [ ] **R12 图路径观察读数（可选增强，非门禁）**：利用现成
      `first_appearance_set` 闭包——正文新出现且不在已知集的**记号样 token**
      只计数不拦截（复用 first_seen_unintroduced 相邻思路，命名如
      `notation_outside_known_set`）；无概念面的传统路径不配（j07 同款切分）。
- [ ] 开关 `NOMI_LEARNING_CONTENT_DISCIPLINE_V2=off`：四条新条款不进装配
      （= j07 后提示词，off 臂）。
- [ ] 条款覆盖短语断言：j07 模式（const contains 测试）加四条关键短语。

## 验收

- **单测**：开关生效测试（off 臂装配文本无新条款短语、on 臂四条全在）；
  R12 观察读数红绿（若做：种子正文含已知集外记号 → 计数 ≥1；全在场 → 0）。
- **smoke content 档双臂（开关臂）**：硬判据——两臂 EXIT 既有判据不劣化；
  on 臂观测存档提示词含四条短语、off 臂不含（对 `calls.jsonl` 机械 grep）。
- **人读对照卡（效果判据，两臂必须都跑）**：同 fixture 课时双臂各一次真机
  采样，按四条款逐条判：R10 首讲抽象对象是否有具象替身/规则是否配「为什么」、
  R11 情境语域是否匹配声明受众、R12 记号是否即兴、R13 例题数字能否推出断言
  ——✅/❌ 并排两臂落 Resolution；**既有 j07 计数器（naming/narrative 系列）
  on 臂不得劣化**（回归护栏）。
- `cargo test -p nomifun-learning` `-p nomifun-ai-agent` 全绿；typecheck 存量。

## 非目标

- 不建 R12/R13 机械门（learnhub 亦未建；机械检测留雾区，回放套件解禁后
  第一批消费方）。
- 不加受众输入字段/向导改动（B 组产品裁决范围）。
- 不动金样三闸资产面（本票只动 Rust 常量块与 agent 系统提示词，k06 才动
  结构资产文件）。

## 备注

learnhub 的配套「病历」思路（真机产物录制存档）在 allo 的等价物 =
ArchivingCompleter 观测存档 + 本票对照卡——效果判据落档可追溯，不引回放
基建。

## Resolution

**已关（2026-10-23，`cd28da39e`；读数面 `dd95ebe83`）**。

- **两管线写作面**：传统管线 `CONTENT_DISCIPLINE_BLOCK` 追加 R10/R11/R12/R13
  四条（`content_discipline_block()` 装配）；图管线 `GENERATE_LESSON_AGENT_SYSTEM`
  成色段同义中文四条（`CONTENT_DISCIPLINE_V2_CN`）；`section_outline_system` 加
  情境取向句（`audience_planning_clause()`）。
- **R11 输入面**：不加受众字段/向导/DB（B 组产品裁决）；条款锚定既有程度声明与
  目标文本，声明非低龄受众时情境取该受众生活域。
- **开关**：`NOMI_LEARNING_CONTENT_DISCIPLINE_V2=off` → 四条与规划句均不装配
  （= j07 后提示词）。
- **单测**：开关两臂（off 臂四条短语全无、j07 基线条款保留；on 臂全在）+ agent
  系统提示词四条短语在场。
- **双臂（开关臂）**：content 档 on 四条存档计数 1/1/1/1、off 0/0/0/0，EXIT 均 0。
  人读（同 fixture 各一次，读落库节正文）：R10 on 臂 s1 先给「两只股票日内
  小幅挪动 vs 涨 4%/跌 3%」情境量 + 分布图、公式推后，off 臂 s1 抽象框定后直接
  上公式（⚠️）；R11/R12/R13 两臂均未违规，R13 算例逐式复核通过。j07 观察条
  两臂同口径无劣化。读数见 [verification-cards.md](../assets/verification-cards.md) §k05。
- **R12 观察读数**：可选增强未做（与 R12/R13 机械面同留雾区，map Not yet specified）。
- 非目标遵办：不建 R12/R13 机械门、不动金样资产。
