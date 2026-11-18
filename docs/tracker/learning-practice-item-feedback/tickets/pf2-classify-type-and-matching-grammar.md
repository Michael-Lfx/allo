---
id: pf2
title: 判别题专有题型 classify + matching 候选语法写作面指导
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

匹配题生成出右列近重复候选（两个「栈区」描述语义等价字面差两字），精
确串判分下教学正确的作答被判错。原 pf2 方案（机械查重门 + 泛泛契约句）
**治不了本例**（两候选字面 distinct 恰能过字面查重；契约句是概率性
的）。研究文档（learnhub docs/research/xuexiguan-skill-deep-dive.md
§5/§6/§8）给出的正确定性：这是**判别模型的探针设计问题**——探针价值
在区分度，「堆砌同义句」被点名为非修法。怎么从题型结构上断根？

## 已核验事实（2026-11-18 取证 + 研究文档对读）

- 取证：matches[0]/[2] 语义等价、字面差「随/内」两字（非判别维度的字
  眼差异）——探针区分度为零，测的是眼力不是「值→栈/堆」的判别模型。
- 判分：逐位置精确串比对（progress.rs Matching 臂），二元 0/1。
- 契约（lesson_content_loop.rs:227/:1612）：右列「一一对应数组」无可
  区分性约束。
- 研究文档对读：§6 负例选取学——候选/负例应是**类别标签或最小对**（只
  差一个判别条件），「堆砌同义句」非修法；§8-4 先例（learnhub）——
  成色需求**走指导不加机械门**；§5——判别能力的探针 = 输入空间对象 →
  类别归属判断。
- ActivityKind 现九种（single_choice/multi_choice/true_false/
  fill_in_blank/numeric/ordering/matching/reflection/open_question），
  无判别归属型。

## Terms（方案草案，施工前过裁决）

- **R-a（主修）专有题型 `classify`（判别题）**：
  - 形态：prompt 给一组具体对象（左列），作答 = 每个对象归入**判别维
    度的类别集合**（右列=短规范标签）。
  - **类别来源放宽（2026-11-18 用户裁决）**：不硬性要求「来自节点
    teaches」——优先沿用节点已教概念（如 栈区/堆区），**允许练习局部
    自拟短类别**（如判别维度本身的状态类别）。硬约束只有三条：
    ①短（规范标签，非描述句）；②normalize 后 pairwise distinct；
    ③语义可区分（差异落在本节教授的判别维度上，如 创建方式/生命周
    期）。「类别来自知识结构本身」原表述过于严格——大量合法分类的
    类别轴是练习局部自拟的。
  - 结构性消除缺陷类：候选=短标签（非模型自由发挥的长描述句）——近
    重复长句候选在题型上不可能再出现。
  - config 形状：`{ objects: string[], categories: string[], answer:
    string[] }`（answer[i] = objects[i] 的类别，逐位置精确比对公平——
    类别是短规范标签，无近重复空间）。
  - 活动审计门沿用（objects/categories/answer 形状校验 + categories
    normalize-distinct 机械断言）。
- **R-a2（新增，2026-11-18 用户裁决）旧 matching 废弃**：
  「options 左列 + answer 自由右列数组」的 matching 题型从**新生成契
  约**移除（:226 题型清单 / :1612 字段规则）——自由右列无好解决方案
  （近重复候选 × 精确判分结构性误判，查重门管不住语义近重复）。判别
  场景由 classify 承接；存量课时 matching 保留渲染/判分/复习兼容
  （向后兼容，只断新增）。若未来出现「右列天然规范可区分」的联结配
  对需求，再以受约束形态另立票。
- **R-b（写作面指导，不加门）**：matching 已废弃（R-a2），候选语法指
  导转入 classify 的生成指导——对象选取与类别轴设计照研究文档 §6 负
  例选取学：近邻正负例（只破一个判别条件）、对比例组集合关系、**「题
  目应只增加可控差异，避免把大量新先修知识引入验证，使失败无法定位」**
  （textbook-teaching 验证题设计原文）。机械查重（categories
  normalize-distinct 断言）随 R-a 审计门保留。
- **R-c 判分与作答面**：classify 判分=逐对象类别精确比对（短标签下公
  平）+ 与 pf1 逐项结果透出天然合流（每对象一行红/绿）。
- **R-d 不做**：判分语义化/LLM 裁决近似匹配（新不可控面）。

## Acceptance

- classify 题型：config 校验（categories distinct、answer 对齐）+ 判
  分单测 + ActivityInput 渲染测试（对象行→类别 select）+ 审计门单测。
- 契约文本变更带既有契约测试跟进；smoke EXIT 0。
- 真机：新生成的判别题右列为短标签（随波欠账走查）。

## Resolution

（未结案）
