---
id: e5
title: "题面小项——difficulty 消解、reflection explanation 三保险、密度锚补强"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# e5: 题面小项（difficulty / explanation / 密度）

**What to build:** 三个 E2E 暴露的软缺陷：①activity 的 difficulty 模型全没
写（5/5 缺）；②reflection 的 explanation 时有时无（课 1 空 / 课 2 有 55 字）
；③首课 4 节 1481 字偏薄（25 分钟课）。按 learnhub 的「结构消解 + 三保险 +
间接密度锚」三案收口。

## allo 现状（E2E 实锤）

- difficulty：生成提示词要求「difficulty: 1|2|3」但模型 5/5 未写；
  `validate_shape` 仅 `if let Some` 校验 1-3（models.rs:960,1253）——缺省合法
  所以静默漏过；下游 quiz 难度分层（q2 每节点一题带 tier）退化。
- reflection explanation：契约 `explanation: String` 非空语义与提示词条款在，
  但校验不咬（空串过闸）；无 schema/自检双保险。
- 密度：R8/意图画像条款已在（六波 C 线落地）；首课仍薄——缺的可能是画像
  输入面（该课节点 est=25 → 画像应给「操练整合」级雄心，实测轻）。

## learnhub 参考（Reference 表）

| 件 | learnhub 路径 | 取什么 |
| --- | --- | --- |
| difficulty 结构消解 | `question-regenerate.md:35`、`quiz-variant.md:31`（「难度与学习点内容相当，以骨架行的难度档为准」=输入锚）；`content-plan.md:28,47` + `json_contract.rs:880`（内容 difficulty 1-5 由计划面必填产出）；`engine/questions/facade.rs:294-301`（引擎铸值注入出题输入行）；契约 §2 字段表**无 difficulty 列**；v1.8①（tier 替代单值难度：「单值难度零区分度」） | 难度**不进题面输出契约**——上游铸值、输入行注入；allo 对应 = activity difficulty 从节点/计划面铸值注入或干脆按 tier 消解，不指望模型输出 |
| explanation 三保险 | `question-generate.md:27,32`（schema required + 「开放题也必须写 explanation」自检句）、`question-regenerate.md:33`、`quiz-variant.md:29`、`json_contract.rs:208`（required 表）、`judge.rs:219-225`（评审输出侧非空机械校验） | 三层：schema required + 交卷自检句 + 机械校验；allo 对应 = validate_shape 对 AI-graded 题也咬 explanation 非空（或至少 reflection），提示词补自检句 |
| 密度间接锚 | `node-growth.md:5,31`（est 15-30 目标/60 拆节点、「上游估值质量直接决定内容铺开或砍雄心」）、`content-write.md:45`（R8 例题三到六个量级、不要一例带过）、`content-plan.md:24`（练习占大头至多五道）、`content-write.md:47`（规划即雄心）、契约 v2.0（「语义纪律不是机械门」定调） | 不加字数下限机械门；密度靠 est 质量 + R8 例题数 + 意图画像；allo 检查点 = 该课节画像档位是否真的吃到了 est=25（输入面读数） |

## 实施要点

- [ ] **difficulty 消解**（推荐 learnhub 案）：activity 输出契约去掉 difficulty
      字段要求；难度由上游注入——课时题在装配时继承节点/节 tier（或 content
      计划面的档位），quiz tier 取数改读注入值；serde default 容存量行。
      *（备选保守案：保留字段但提示词加自检句 + validate 警告读数。票内
      grilling 二选一，推荐前者。）*
- [ ] **explanation 三保险**：validate_shape 对 reflection/open_question 增
      explanation 非空校验（装配/重写两面同咬）；课时提示词补「交卷自检」
      句式（每题当考生重解一遍、AI 批改题也必须写解析）；存量空 explanation
      行读侧容错不变（serde default 语义已对）。
- [ ] **密度读数**：不立机械下限（learnhub 定调照搬）；smoke content 读数行
      增「节均字符量 + 例题数」并记录基线；若仍薄，回查意图画像输入面
      （est×档位行是否真注入——接六波 smoke 判据②的观测存档 grep）。
- [ ] 金样/条款断言随批；`cargo test -p nomifun-learning` 全绿。

## 验收

- 红绿样：reflection 空 explanation 被校验打回；difficulty 消解后 quiz tier
  取数不空（存量行容错测试）。
- 真课对照卡：消解后连续两课 difficulty 注入率 100%、reflection explanation
  非空率 100%、节均字符量基线入档。

## 非目标

- 字数/节数机械下限（learnhub 明拒）。
- tier 五档制（learnhub v1.8① 的低/中/高样例密度档——评估面另议）。
- quiz 变式面改动（q3 已关）。
