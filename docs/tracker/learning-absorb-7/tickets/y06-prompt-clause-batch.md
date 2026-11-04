---
id: y06
title: "内容/题库提示词条款批——干扰项五型、完备性三面、材料学六条"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# y06: 提示词条款批

**What to build:** 把 learnhub #201/#202/#203 的三组写作/出题条款，按 allo 的
内联提示词结构落成 **英文写作取向块**（agent 线为中文孪生块）：干扰项五型、
完备性三面、材料学六条。**不加新机械门**；全部挂
`NOMI_LEARNING_CONTENT_DISCIPLINE_V2` 的 on/off 双臂，off 臂逐字回退旧文本。

## 现状（2026-11-04 实码核实）

- 内容/题目提示词是 Rust 内联常量，不是文件资产：
  - `generation/mod.rs:83` `LESSON_SYSTEM`（整课题库）、`:142`
    `LESSON_ACTIVITY_SYSTEM`（单题追加）。
  - `generation/lesson.rs:855` `section_outline_system`、`:911`
    `section_body_standard`、`:932/:948` 苏格拉底/费曼变体；写作纪律块
    `:788` `CONTENT_DISCIPLINE_BLOCK`（常开）与 `:821`
    `CONTENT_DISCIPLINE_V2_BLOCK`（开关）。
- agent 线孪生：`nomifun-ai-agent/src/lesson_content_loop.rs:67`
  `CONTENT_DISCIPLINE_V2_CN`、`:94` `GENERATE_LESSON_AGENT_SYSTEM`；
  题目规则在 :133-134 与 `ls_patch_activities` 描述 :1153。
- 现状缺口（逐条）：
  - **干扰项五型**：只有「3-5 个互异选项」（mod.rs:110）与填空的
    「近义词/物理相邻量」干扰（:112），没有判联错配/表述转换/集合关系等
    负例语法；变式站复用同一 `LESSON_ACTIVITY_SYSTEM`（activities.rs:351）。
  - **完备性三面**：规划面只有节序列/难度/例题密度/练习边界
    （lesson.rs:895-906）；写作面是「只教本节任务点、只用已教内容」
    （:923-926）——都是上限，不是覆盖下限。
  - **材料学六条**：类比边界仅在费曼风格有一句（:956）；例子判据只到
    「题面→分步→答案」（SECTION_TYPE_MENU）；符号—对象—条件落点、分段
    例组、关系句可还原、例组反模式自查均无。
- 内容/题目提示词**无金样**，只有短语断言
  （`generation/tests.rs:62-109,1460-1540`；agent 侧
  `lesson_content_loop.rs:1552-1640`）。

## learnhub 参考（main@22d016d）

- #201 `8932abb`：`prompts/questions/question-generate.md:23-28`、
  `question-regenerate.md:29`、`quiz-variant.md:26`——负例五型菜单
  （近邻负例只破一个条件 / 部分条件满足 / 判联错配 / 表述转换陷阱 /
  集合关系检查），每型带适用/不适用对；总闸「适用时优先、无自然负例
  不得硬造」；三金样同批。
- #202 `565a2fe`：`contentgen/content-plan.md:32`、`content-write.md:41`、
  `questions/question-generate.md:22`、`quiz-variant.md:25`——完备性三面
  （认得出什么 / 推得出什么 / 怎么推出来）；规划面「缺哪个补哪个」，
  写作面「完备面不缩水」，题库面「两个考向都有份量」。
- #203 `7575d4f`：`content-plan.md:34-39`、`content-write.md:44-49`——
  材料学六条（例子四件齐 / 符号—对象—条件落点 / 类比带边界 / 分段例组 /
  关系句可还原 / 例组反模式自查）。
- 研究档 `464c550` `docs/research/xuexiguan-skill-deep-dive.md`：只作条款
  取舍的评审参考，**不进运行时提示词**。

## 实施要点

### A. 干扰项五型（题库面）

- [ ] 新增 `generation/mod.rs` 常量 `DISTRACTOR_MENU_BLOCK`（英文）：
  五型菜单 + 每型适用/不适用 + 「适用时优先、无自然负例不得硬造」。
- [ ] 在 `activities.rs:197`（整课）与 `:307`（单题/变式）装配时，
  `content_discipline_v2_enabled()` 为真才追加到 `LESSON_SYSTEM` /
  `LESSON_ACTIVITY_SYSTEM` 之后（不把 const 改成运行时串；用局部
  `format!`）。适用域写明：`single_choice`/`multi_choice`/`true_false`
  的诱惑项；`fill_in_blank` 既有「近义词陷阱 + 机械必填 distractor」保持
  不动；`ordering`/`matching`/`numeric` 不适用。
- [ ] agent 线：`GENERATE_LESSON_AGENT_SYSTEM` 题目段（:133-134）追加同义
  中文五型；`ls_patch_activities` 描述（:1153）补一句「负例优先自然，
  无自然负例不硬造」，避免长菜单塞进工具描述。

### B. 完备性三面（规划 + 写作 + 题库取向）

- [ ] `section_outline_system`（lesson.rs:855）Rules 追加（开关 on 才装）：
  「每个节点规划时过三面：认得出什么（名称+边界）/ 推得出什么（结论+
  成立条件）/ 怎么推出来（中间步骤）；缺哪个补哪个，不虚构前提外内容」。
  写作面在 `CONTENT_DISCIPLINE_V2_BLOCK` 加对偶「完备面不缩水」。
- [ ] **与现有教学边界并置，不得冲突**：明确「在已教边界内查覆盖缺口」，
  「只教本节任务点 / 只考正文教过的」上限条款保留；禁止诱导超纲推理。
- [ ] 题库面在 `LESSON_SYSTEM` 加一句覆盖取向（「认一认/推一推两个考向
  都要有份量」），**不变成题量配额**（allo 有 9 种题型 + 强制多样性 +
  reflection/open 配额）。
- [ ] 变式站 `quiz-variant` 同源：`LESSON_ACTIVITY_SYSTEM` 的 focus 路径
  自动继承；不另写第二份。

### C. 材料学六条

- [ ] 进 `CONTENT_DISCIPLINE_V2_BLOCK`（开关 on 才装，off 臂旧文本）：
  1. **例子四件齐**：对象/条件/结果/作用过程，禁标签式；操作示范改为
     「行动 + 可观察反馈 + 调整依据」，不套四件。
  2. **符号—对象—条件落点**：公式/定义首现逐项对号；明确「不新增术语」，
     与 R12（术语在已知集内）并置，不放宽 R12。
  3. **类比带边界**：对应什么/不对应什么；费曼风格原有句保留为风格内加码。
  4. **分段例组**：分界两侧各一例 + 压线例 + 切换条件；线性关系不硬凑；
     与「never pad / 3-6 例题密度」并置，禁止互相加码。
  5. **关系句可还原**：保留方向与条件，相关不写成因果、性质不升级为充要
     （放 R13 邻条）。
- [ ] **例组反模式自查**（全正例/无关对比物/一例代表全部分支/背景冒充
  辨认特征）：不进运行时提示词，落到
  `docs/tracker/learning-quality-fixes/blueprints/` 的评审清单/金样评审
  说明，避免退化成 checklist、零新机械门。

### D. 孪生块与断言

- [ ] agent 线 `CONTENT_DISCIPLINE_V2_CN`（:67）同义追加材料学五条 +
  完备性写作面；`GENERATE_LESSON_AGENT_SYSTEM` 的规划段追加三面与分段
  例组。保持「两处手写、各随其风格」的非对称孪生，不做共享常量（跨 crate
  依赖方向 ai-agent → learning，强行共享会把 prompt 挪错层）。
- [ ] 断言：`generation/tests.rs` 扩
  `richness_clauses_are_present_in_generation_prompts` 与新 on/off 测试
  （off 臂断言新短语不出现、旧短语仍在）；agent 侧
  `lesson_content_loop.rs:1552` 扩 R15/R16 类短语断言。
- [ ] 无 i18n 变更（模型面英文/中文提示词，不是用户可见文案）。

## 验收

- `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿；on/off 双臂
  短语断言通过。
- 真机（串行）：`NOMI_LEARNING_CONTENT_DISCIPLINE_V2=on` 生成 1 课内容 +
  题库，人工走查：选择题干扰项至少覆盖两类负例且无硬造；例题含对象/条件/
  结果/过程；类比有边界句；分段关系有两侧例；关系句方向正确。off 臂跑
  1 课，确认旧行为（条款不出现）。
- smoke content 档 `EXIT=0`；如实记录采样差异（提示词改动不承诺统计显著）。

## 非目标

- 不加内容质量机械门/validator（alo 哲学是形状门 + 写作取向）。
- 不引入 learnhub 的 replay 夹具/金样基础设施（本票只扩短语断言）。
- 不把研究档的类型学术语写进提示词/UI/判分。
- 不改题型菜单、题量预算、多样性/配额规则。

## 风险

- 条款互相加码：完备性三面 vs「只教本节任务点」、分段例组 vs「never pad/
  密度」、符号落点 vs R12——每处必须显式并置「不新增术语/不超边界」。
- 提示词变长会挤占输出与注意力；把五型菜单放题库面、材料学放写作面，
  不同时塞进所有 stage。
- 无金样保护：改一句只靠短语断言；实现时把每条新增条款配一个唯一锚点
  短语，避免误删无感知。
