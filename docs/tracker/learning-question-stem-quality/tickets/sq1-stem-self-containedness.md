---
id: sq1
title: 题面自足性：契约条款两线同步 + 审计词面门 + agent 线 evidence 条款恢复
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

题目引用「演示」却不重述演示内容：题面「把演示中值建立与失效的先后顺
序排好。」+ 五个剧情碎片选项，本节内靠回滚三屏勉强可答，进刷卡后完全
不可答。且这是**机械可检的失败**（指代词裸现题面），不该靠概率性条款
碰运气。

## 已核验事实（2026-11-19 对码）

- 全部契约面（agent 线【活动契约】lesson_content_loop.rs:293-307 +
  传统线 LESSON_SYSTEM_TAIL generation/mod.rs:187-203）**无题面自足条
  款**。代码里唯一「自足」概念是 SVG viewBox（图形自足，非题面）。
- 审计门（lesson_draft.rs:1149 audit_findings）finding 种类：节缺失/
  形状违/题面空/题面雷同/scope 边界/viz lint——**无指代检测**。三道闸
  全放行。
- evidence 回归：传统线 evidence_missing 硬门（generation/activities.rs:
  340，objective 题必带 ≤120 字逐字引用）在 agent 线**整个丢失**——
  本课 8 题 evidence 全 NULL，判底揭底无源。
- 答案键连带风险：演示 mermaid 把「订单名」写两遍（第 1 步 vs 第 8
  步），落库答案键排最后——裸指代题的答案键与演示自身叙事冲突（map
  取证节）。
- 刷卡卡面=题面+选项+课时标题（review.rs:57 + ReviewSession.tsx:164），
  节正文不在任何字段——自足是卡面可答的唯一保障。
- **出题生产面共三条 LLM 链（2026-11-19 影响面清点）**，契约与闸必须三
  面全覆盖：① agent 线整课面（【活动契约】）；② 传统线整课面（LESSON_
  SYSTEM_TAIL）；③ 单题 LLM 面（lesson_activity_system_prompt）= AI 手
  动加题 + **检测轮变式站**（generate_quiz_variant → generate_lesson_
  activity 复用，quiz.rs:888）——**变式以旧题面为换皮底，旧题面裸指代
  则变式遗传裸指代**，单题面 validator（validate_generated_activity）
  必须同步加校验。
- 手动表单加题（create_lesson_activity 直 INSERT）无 LLM 无审计，闸拦
  不住——sq5 删除兜底，如实留痕不做机制面。
- 存量证据：evidence 全空课时 3/5（竞态条件/值存储位置/消息传递 15
  题）——「参数传递」5/5、「判定类型检查时机」6/8 有值，**回归按代次
  发生**，agent 线契约丢件实锤。

## Terms（方案草案，施工前过裁决）

- **R-a 契约条款（两线同步 + 单题面）**：题面必须自足——作答所需情境
  材料（代码段/数据/剧情/图示内容）在题干或题面附块中原样给出；禁止
  裸指代「演示中/上例/前文/本节/如图所示/例题里」指向课内其它节而不
  重述。三面覆盖：agent 线【活动契约】、传统线 TAIL、单题面
  LESSON_ACTIVITY_SYSTEM_BASELINE（AI 加题+变式站同源）。一致性测试
  钉住（pf5/F1 同源先例）。
  变式站继承题留痕：旧题面坏 → 变式换皮也坏——闸在单题面 validator
  拦新产变式；存量坏题面的变式由 sq5 删源题兜底。
- **R-b 审计机械门**：lesson_draft.rs 新 finding kind
  `activity_prompt_dangling_reference`——保守词表（演示中|上例|前文|
  如图所示|例题中|上一节|前一章）扫 objective 题面 → **danger 级** +
  修复指引「把所引材料在题面重述」。agent 修复轮按 findings 自修，门
  与修复环闭合。单题面同步：validate_generated_activity 加同款校验
  （拦 AI 加题+变式新产）。误伤裁决点：题面自带完整材料后「对上例判
  断」类合法用法——倾向不豁免（指引改措辞即可），施工时定案。
- **R-c evidence 条款 agent 线恢复**：objective 题必带 evidence + audit
  校验（传统线 evidence_missing 同款移植进 audit_findings，danger）。
  判底揭底有源；agent 线回归实锤留痕。
- **R-d 刷卡卡面不加上下文（后置候选不做）**：自足题面 + 既有课时标题
  已足；存量坏题归 sq5 下架。少加面。

## Acceptance

- 三面一致性测试：三处契约（agent 整课/传统 TAIL/单题面）删裸指代禁
  令句即红。
- audit 门单测：词表命中/豁免边界/修复环（命中→改写→过闸）。
- 单题面 validator 单测：AI 加题与变式新产裸指代题被拒（变式继承场景
  一例）。
- agent 线 evidence 校验单测（缺 evidence → danger → 修复环补齐）。
- 真机 smoke 出题抽查：新生成题面零裸指代、objective 题带 evidence、
  变式站对旧坏题面的行为留痕。
