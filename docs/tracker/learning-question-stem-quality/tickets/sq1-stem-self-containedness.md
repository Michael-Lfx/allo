---
id: sq1
title: 题面自足性：契约条款两线同步 + 审计词面门 + agent 线 evidence 条款恢复
type: task
status: closed
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

## Resolution（2026-11-19 施工落地）

### 落地哈希

- feat commit：`795a04425ecb97f333486ab2668e4c266887df4d`（wave/sq1，
  基点 4074e38fc）
- docs commit：本节所在提交（追加于 feat 之后）。
- 合入集成：`git merge feat/learn1116` → merge commit
  `bad8c3c64`（Merge branch 'feat/learn1116' into wave/sq1，零冲突；
  合入 sq5 的删除路由 + 前端删除入口与 i18n 双侧键）。

### 裁决点落地

- **误伤豁免（R-b，票面倾向「不豁免」）**：照倾向落地——词表纯
  contains 命中、无豁免分支；题面自带完整材料后「对上例判断」类合法
  用法同样命中，处置=修复指引「把所引材料在题面重述后再提问」（重述
  后自然不再用指示语开题）。钉在
  `dangling_reference_prompts_audit_as_danger_and_clean_prompts_stay_silent`
  与词表常量 `DANGLING_REFERENCE_TERMS` 的 doc 注记。
- **机械门扫描面（R-b）**：objective 题面 only（票面口径）——
  reflection/open_question 题面不扫；单题面 validator 同款口径
  （objective only，reflection 不核），两处语义一致。
- **evidence 落点（R-c）**：审计既有 `evidence_missing` warning 升
  **danger**（两线共库审计，agent 线唯一闸）+ 位置点名（修复轮
  update_activity 按位补齐）；agent 线【活动契约】补回「objective 题
  必带 evidence」条款（契约半恢复，传统线 TAIL 原有同款条款未动）。
- **真机 smoke（Acceptance 末条）**：本工位无 provider 凭据，未跑——
  如实留痕。机械面（门/环/三面一致性/变式拦截）已由单测覆盖；真机
  抽查（新生成题面零裸指代、变式站对旧坏题面的行为读数）留给发布面。

### 施工纪要

- 三面契约：agent 线 `ACTIVITY_STEM_SELF_CONTAINED_CN`（无开关恒在，
  同命装配注入【活动契约】）；传统线 `STEM_SELF_CONTAINED_RULE`
  逐字嵌进整课 TAIL 与单题/变式 BASELINE。一致性测试两道：
  learning 宿 `stem_self_contained_rule_is_carried_by_both_traditional_faces`
  + ai-agent 宿
  `stem_self_contained_clause_spans_all_three_generation_faces_and_matches_gate_terms`
  （后者兼钉契约↔门词表逐词对齐与修复环映射行，pf5/F1 同源先例扩面）。
- 机械门：audit finding `activity_prompt_dangling_reference`（danger，
  词表=演示中|上例|前文|如图所示|例题中|上一节|前一章，与
  `prompt_dangling_reference_hits` 同源共用一张表）；
  `validate_generated_activity` 同款校验置证据逐字核之前（AI 加题+
  变式站新产都拦；变式继承场景单测钉住：旧题面「上例/前文」遗传进
  变式即拒）。
- golden 已知变更留痕：`GOLDEN_LESSON_SYSTEM` /
  `GOLDEN_LESSON_ACTIVITY_SYSTEM` 各嵌题面自足条款一句（逐字 =
  `STEM_SELF_CONTAINED_RULE`，TAIL 在 evidence 行后、BASELINE 在
  evidence 行后），两常量 doc 注明 sq1 已知变更——**这是「已知变更」
  不是破坏**。
- 修复环：`REPAIR_LESSON_AGENT_SYSTEM` 增两行修复动作映射
  （activity_prompt_dangling_reference / evidence_missing →
  update_activity 按位重写/补引用）。
- 波及面如实修复：evidence 升 danger 后，两 crate 的发布面 fixture
  （service finish 门 4 测 + lesson_draft finish_gate + ai-agent
  scripted 循环 `valid_activity_ops`）objective 题补 evidence——已知
  变更，非放宽。

### 检查记录

| 命令 | 结果 |
| --- | --- |
| `cargo test -p nomifun-learning` | 1000 passed / 0 failed（全改后）；合入后再跑 1001 passed / 0 failed（含 sq5 新增服务测试，合入态全绿） |
| `cargo test -p nomifun-ai-agent` | lib 1175 passed + 16 个集成测试目标全过 / 0 failed（清盘后冷重建全绿；合入后复跑同全绿） |
| `cargo fmt -p nomifun-learning -p nomifun-ai-agent -- --check` | clean |
| `bun run check`（UI） | 未跑——本票零 UI 改动（R-d 不做，前端未触碰） |

### 机器事故留痕（不阻塞，透明留痕）

- `[build] build-dir = "{workspace-root}/build.noindex"`（.cargo/
  config.toml）使每个 worktree 独立积累构建缓存；票面给的
  `CARGO_BUILD_DIR` 环境变量对该配置**不生效**（实测 rustc 仍落
  worktree build.noindex）。本工位构建把系统盘顶到 100% 满
  （rustc "no space on device"，连锁出现 rmeta 损坏假象）。
- 处置：删除本 worktree（allo-wt-sq1）与**已完成并合入**的 sq3/sq5
  worktree 的 build.noindex/target（均为可重建构建产物，mtime 静止
  >1h；sq1/sq3/sq5 三处共释放 ~26G）。主仓 `allo/build.noindex`
  （活跃中，属并发代理）未触碰。共享缓存指令建议后续修正为对该配置
  真实生效的方式（如 `CARGO_TARGET_DIR` 或 config.local.toml 覆写
  build-dir）。
