# Map: quiz 结算线——检测/quiz 从条款到落地

<!-- wayfinder:map -->
<!-- 状态：**已到达目的地（2026-10-16）**——q1（034e90e10）/ q2（dff1d8754）/
      q3 全部关闭并推送 origin/feat/learn0928。quiz 线全链就位：条款（j05）→
      纯函数+K 触发（q1）→ 瘦回合载体+结算落账（q2）→ 弱先/变式/校准（q3）。
      后置未迁项见 q3 Resolution §7（g4 数据面/归因通道/记录文件面）。 -->
<!-- 前情：j05 已落 quiz 恒非负条款与 `quiz-completed` kind 枚举（账本+验证器就位）；
      j09 已落 findings 留痕（quiz 回合可产出 findings 作为修补输入源）。
      learnhub 侧 quiz 线已完整实现（不是设计稿）——本波是移植已知物，风险低。 -->
<!-- 来源：learnhub `C:\Users\test\Desktop\my\learnhub`，关键参考：
      src-tauri/src/engine/questions/settle.rs（settle_quiz 分级函数），
      src-tauri/src/domain/quiz.rs（K=6 触发/预算 15/弱先排序/变体），
      src-tauri/src/app/sessions.rs（quiz 会话命令面/脱敏投影），
      src-tauri/src/domain/sessions.rs（SessionRecord 首写为终），
      src-tauri/src/engine/questions/grading.rs（确定性判分零 LLM）。 -->

## Destination

学习域 XP 结算三线的最后一线——检测/quiz 结算落地：按 K 计数触发 quiz，学习者作答
银行旧题（+LLM 变体），确定性判分后按准确率分级结算（`quiz-completed` 落
`learning_xp_events`），QuizRecord + 校准点落盘，N/K 进度读数暴露。账本与条款已在
j05 就位（恒非负、幂等键、kind 枚举），本波补生产者。

## Notes

- **依赖**：`learning_xp_events` 账本（j05）、`validate_settlement` quiz 恒非负条款
  （j05）、确定性判分 `evaluate()`（allo 已有，9 题型全覆盖零 LLM）、findings 留痕表
  （j08，quiz 回合可产出 findings 作为修补输入源）。
- **触发条件来源**：g4 裁决"任一进入实施"——quiz 线是会话命令面的第一个消费者，
  q2 的载体形态（瘦回合制 vs 完整会话实体）在 q2 票面裁决。
- **量纲**：quiz 结算按工作量当量 × 准确率分级，**无 3 分钟下限**（这是 quiz 线与
  node/review 阶梯的关键区别——learnhub settle_quiz 独立于 settle_node/review）。
- **纪律沿用**：迁移从 081 起；一票一会话；Resolution 收口 + Conventional Commits；
  learnhub 参考实码是最高事实源——先读参考再动 allo 代码。

## Tickets

| 票 | 标题 | 依赖 |
|---|---|---|
| q1 | settle_quiz 分级函数 + K 计数触发 + N/K 读数 | 无（纯函数面 + 账本回放） |
| q2 | quiz 回合载体 + 结算落账 + QuizRecord | q1 |
| q3 | LLM 变体生成 + 弱先排序 + 校准点与读面 | q2 |
