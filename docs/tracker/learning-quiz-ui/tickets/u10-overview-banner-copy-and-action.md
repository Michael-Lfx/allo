# u10: 感知横幅文案与行动修正——「检测到期」措辞 + 直开检测会话

## Status

施工中（2026-11-12 开票；用户桌面走查暴露）。

## 已核验事实（2026-11-12 对库）

- 图课程 `quiz_rounds = []`、`quiz-completed` 事件 0 笔——横幅 6/6 due 行为
  **正确**（该课确实未测）；用户「做过了」= 昨天测的是传统课 + 今天中午验收
  探针 `--graph --fresh` 重灌重置（操作方为集成方，已认领）。
- 「检测到期」措辞系 grill Q3 推荐文案（quizOverviewTitle），积累制触发改用
  时间语义词属误导；行内「已到期」标签同族。
- u7 横幅行点击 = `navigate(/learn/{id})` 只落课程页；检测入口在工作区底缘，
  用户预期「点横幅 = 开测」。

## Terms

- **R1（文案）**：`quizOverviewTitle`「检测到期」→「检测已就绪」（en "Quiz
  ready"）；`quizOverviewDueTag`「已到期」→「已就绪」；en 对称同步。积累制
  语义在横幅行现有读数句（「距上次检测已积累 N/K 次学新结算」）已自明，
  不另加句。
- **R2（行为）**：横幅行点击 = **直开该课程检测会话**——置
  `quizSessionCourseId` + `navigate(/learn/{course_id})`，QuizSession 经既有
  `renderQuizSession` 在工作区上立即挂载（传统/图分支均已接线）；不再只落
  课程页让用户自己找入口。409 竞态由 QuizSession 既有 onConflict 通道兜。
- **R3（回归面）**：横幅渲染测试更新（新文案值 + 行点击断言
  navigate + 置位）；learning 回归绿；typecheck/i18n 绿；theme/dead-css 干净。
- **R4（不做）**：横幅结构不动（聚合/排序/有 due 才渲染语义保留）；CONTEXT.md
  「检测」词条不动（术语不变，只是 UI 措辞）。

## Acceptance

- 渲染测试双 locale（新文案值 + 行点击双断言）；全闸绿。
- 真机：学习首页横幅显示「检测已就绪」→ 点行 → 直开检测会话。

## Resolution

（施工后收口）
