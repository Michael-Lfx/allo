# ADR-0005: Mastery 收敛为记忆稳定度与练习证据的读侧派生

Status: Accepted（口径先行锁定；实现排期学习域迁移 Phase 2，见
`docs/research-learnhub-migration.zh-CN.md` §5）

Date: 2026-09-09

## Context

- 现行 mastery 是落盘的概念级 EMA（`learning_mastery_states`，旧值·0.7 + 新分·0.3），
  复习流答对/答错也回灌同一 EMA。
- 失真有二：**饱和**——一次全对首学即把 EMA 推到高值并长期停留；**无记忆分量**——
  学完一个月不复习，EMA 纹丝不动，而真实记忆已大幅衰减。词汇表对 Mastery 的承诺是
  「忠实表达学习者对该课时的实际掌握程度」，现口径做不到。
- learnhub 已为同一问题立 ADR-0007 并收敛到读侧派生（`0.7·min(1, S/60) +
  0.3·练习EMA`，S_MASTER = 30 天，不落盘），并把「正确率」立为独立词汇。本项目
  词汇表自称与其同源，此为本对齐项。
- 现有消费方：传统课程薄弱前置推荐阈值 0.8（`course.rs`）、前端「全部掌握」判定
  （`ui/src/renderer/pages/learning/model.ts`）。

## Decision

- mastery 收敛为**读侧派生值，不落盘**：`0.7·min(1, stability/60) + 0.3·练习EMA`。
  `stability` 取该课时题目卡 FSRS 状态的聚合代表（代表卡选取默认沿用 learnhub 的
  「到期最早那张卡」口径，实现期可复核）；练习 EMA 沿用 `learning_mastery_states`
  的现有更新机制，但它降级为**练习证据源**，不再是 canonical mastery。
- **Answer Accuracy（作答正确率）** 立为独立度量（答对次数 / 作答次数），只服务
  完成门禁与内容诊断；任何界面不得把正确率当 mastery 展示，反之亦然。
- 两个消费方同步切换到派生值：推荐阈值 0.8 的语义变为「派生 mastery < 0.8 视为
  薄弱前置」；前端「全部掌握」同口径。EMA 历史数据保留作证据，不迁移、不改写。
- 实现排期 Phase 2（复习队列 R 排序与记忆统计稳定之后）：派生函数 + 课时/课程视图
  换算 + 前端口径与 i18n。本 ADR 先行锁定口径，避免实现期再争论。

## Consequences

- 界面数字会普遍变低：学习完成当天的 mastery 从 ≈1.0 落到 0.3–0.5 区间，且随遗忘
  自然回落。这是特性不是回归，但需要文案解释（tooltip/i18n）；learnhub 为同一收敛
  付出的「头部数字变低」沟通成本是已知先例。
- mastery 不再能从单表直读；统计与导出方需走派生函数。
- 0.8 阈值下传统课程的推荐节奏变紧（要更深的稳定度才解锁后续）；首版上线后观察
  推荐量变化再定是否调阈。
