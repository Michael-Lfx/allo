---
id: n4
title: "开放题判分重写 grilling——判底来源 / 失败出口 / 分数形态 / 模型档 / 范围与不对称"
type: grilling
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# n4: 开放题判分重写 grilling（A 线前置裁决）

**What to decide:** #181/#182/#184 移植前裁决。allo 已是 LLM 评审制
（grade_open_answer authoritative fail-loud），本波核心不是「引入评审」而是
**判底质量 = 评审质量上限**（learnhub ADR-0037 核心洞见）+ 评审执行器三重
脆弱修复。九个裁决点带推荐项。

## 裁决点

### D1. 判底来源（最大决定，决定 n5 形状）

allo 开放题 `config.answer` **契约性为 null**（validate_shape models.rs:1054
非 null 即拒）；但 `explanation` **非空必填**（models.rs:928 + LESSON_SYSTEM
「每个 explanation 说清为什么对」）且从未进评审 prompt
（build_open_grading_prompt progress.rs:296 只吃课时标题+摘要）。

- (a) **v1：explanation 判底**——评审输入 = 题面 + 作答 + explanation +
  课时上下文。零契约变更、零迁移、存量题即刻受益（存量全有 explanation）。
- (b) **增强档：参考答案+要点进契约**——learnhub 全形态（answer=评审底稿
  完整规范作答 + points=3–6 条一句话可判定要点），需反转 validate_shape、
  StoredActivityConfig 增要点字段、LESSON_SYSTEM/LESSON_ACTIVITY_SYSTEM
  重写（= allo 版 #184）、存量双轨（无判底题走 (a) 兼容臂）。
- **推荐：阶梯式——n5 落 (a)，(b) 后置独立票**（触发条件 = (a) 真机对照卡
  显示判分质量不足）。learnhub 自己也走了存量兼容（「存量题读侧兼容，
  数据不修」），(a) 不违背吸收精神。

### D2. 失败出口：fail-loud vs 结构化 unavailable

allo 现状 = 评审失败 HTTP 错误、作答不落 attempts（progress.rs:137-144 注释
明言废弃过静默 1.0 兜底）。learnhub = 三态降级链（评审→机械兜底 degraded→
不计分不落账 unavailable）。**机械兜底层在 allo 无存在条件**（无 keywords
存量，D1(b) 未做）——移植链只剩两端。

- **推荐：保留 fail-loud 出口 + 修三重脆弱**（无超时→90s×N=3 重试环带原因
  回灌、无瞬态重试、非 json 通道→complete_json 族）。失败率被重试环大幅
  压低后，剩余失败 = provider 不可用，学习者重试语义清晰；「不计分不落账 +
  分母剔除」在 allo 天然成立（attempts 不落行即不计分，课时完不成即不结算）。
  结构化 unavailable 三态（会话继续+该题跳过+前端重试 UI）后置，触发条件 =
  真机数据显示重试后仍高失败。
- 同时把「绝不静默判错」写进评审器契约（JudgeExhausted 如实上抛，learnhub
  语义照搬）。

### D3. 分数形态：二值 verdict vs 0–10 连续分

learnhub 二值（correct/incorrect + missing_points 互斥）；allo 0–10 连续 +
0.6 及格 + 前端显示 score + is_guessing 消费。

- **推荐：保留连续分 + 增补反馈结构**——评审 schema 扩为
  `{score, feedback, missing_points}`：score≥及格线时 missing_points 必空
  （互斥校验的连续分适配）；missing_points 进 AttemptResult 新字段（前端
  「缺这些要点」展示，i18n 双语）。二值化破坏 AttemptResult 契约/前端/is_
  guessing，收益（schema 简单）不抵。

### D4. 重复提交语义

allo 现状：同 activity 每次提交 = 全新评审 + 新 attempts 行（无折叠无幂等）。
learnhub 有同日折叠 + HeldSession 缓存（缓存对 allo 伪需求——map 对账事实）。

- **推荐：本波不动**，雾区记录（成本读数：每题一次评审调用，重复提交频率
  数据先行）。attempt 语义本就「每次作答一行」。

### D5. EvidenceTier 证据档：不引入（显式豁免）

learnhub 账本档位成对如实记（program/①、judge/③ 错配拦截）保护「自动性
证据纯度」。allo 开放题已被 OBJECTIVE_KIND_SQL_LIST 结构性排除在 quiz/复习/
客观分母之外——纯度已由题型白名单实现。

- **推荐：显式裁决不引入**（不为此动 015 migration 高风险线），票面记豁免
  理由，避免半吊子移植。若有 XP 面顾虑见 D6。

### D6. node XP 分子不对称（现存问题顺带裁决）

`settle_node_completed`：total_questions 只数客观题（xp.rs:62-69）但
answered/passed 子查询**不过滤题型**（xp.rs:70-95）——开放题 passed 抬
ratio 分子不进分母，也进 is_guessing 正确率。

- **推荐：修成对称排除**——AI-graded 题（reflection+open_question）不进
  node XP ratio 的分子分母（它们每课 ≤3、定位是深练反馈不是测量；learnhub
  方向同——不可用/开放题双侧剔除）。本波修对称，行为变化进对照卡
  （同一课时 off/on 臂 XP 读数并排）。

### D7. 模型档：判分开不开思考

learnhub judge = 思考开 + 温度 0（语义裁决档）。allo `complete` 通道
no-thinking 是项目约定（learning_completer.rs:14-19，flash reasoning 槽吃
budget 实证在案）；trait 面 `complete_json_with_thinking` 已有（generation/
completer.rs:124-154，480s/32768 显然过大需另设）。

- **推荐：判分开思考**（每题一次的低频高质量面，值得），走
  complete_json_with_thinking + 专项预算（票内实测定，参考 learnhub 90s×3
  超时环）；无 thinking fallback 到普通 json 通道并在观测档标记降级。
  deepseek-v4-flash 真机双跑（thinking on/off）进对照卡。

### D8. reflection 是否同票

reflection（0–1）与 open_question 共享 grade_open_answer 全链。

- **推荐：同票改**（机制面同改、判底同 explanation、schema 同构——
  0–1 分数形态保留）；避免双轨漂移。smoke 判据覆盖两题型。

### D9. 观测与脱敏

allo 判分生产零观测档；ArchivingCompleter 冒烟档全文落盘零脱敏——判底
（explanation）进 prompt 后冒烟档会携带参考答案。

- **推荐：冒烟档加脱敏归档器**（learnhub redact_prompt/redact_response
  占位替换移植：判底/要点/作答占位，feedback 原文保留），smoke grade 判据
  含「判底原文不得出现在任何归档 prompt」回读断言；生产观测维持现状
  （任务级旁路），judge 调用是否进生产观测留雾区。

## 验收

- 九裁决点全部落 Resolution（推荐项默认裁决标注可复议）。
- D1(b)/D2 结构化三态若被裁决后置，票面写明触发条件。
- n5 票面按裁决更新后才能开工。
