# u3: 测验会话容器（开测 → 逐题判分 → 全卷收集）

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（容器骨架）**：`QuizSession.tsx` + `hooks/useQuizSession.ts`，骨架
  照 `ReviewSession.tsx`/`useReviewSession.ts`（538 行容器 + 243 行 hook
  的既有形态）：打开即 `quizStart(courseId)` → 回合态机
  `{ round, index, answers, feedback }`；入口由 u2 的 `onStartQuiz` 接线。
- **R2（题面呈现）**：逐题渲染 `QuizRoundQuestionView` 全字段——
  `prompt` 走 InlineMarkdown 管线（题面契约允许 $…$ 与内联 markdown）；
  `options`/`matches` 交 ActivityInput；`node_name` 作题源节点标注（跨节点
  交错序 = 后端弱先序，**UI 不重排**，呈现节点归属即可）；`difficulty`
  档位 1–3 可选呈现（None = 未铸值，不占位）。**作答控件直接复用
  ActivityInput**（九题型；reflection/open_question 不会出现在题源，呈现
  面防御分支如实处理即可）。
- **R3（逐题判分）**：提交即 `quizGrade(courseId, roundId, activityId,
  response)` → `QuizGradeFeedback` 呈现：对/错标识 + 判底答案 + 解析
  `explanation`（InlineMarkdown）+ 证据 `evidence`（有则呈现，InlineMarkdown）；
  反馈期间作答控件 disabled；「下一题」推进。
- **R4（持答案红线）**：题面只渲染 QuestionView 字段；GradeFeedback 只在
  判分后随响应出现并只在当前题呈现——UI 不得预取、缓存跨题或提前展示。
  会话关闭即丢弃全部反馈态。
- **R5（全卷收集）**：`answers: Record<activity_id, response>`——判分时
  的 response 原样回收（与 complete 契约一致：判分零落账、结算在 complete
  全卷面）；**全题作答才允许进入 complete**（前置校验，后端缺题拒绝是
  兜底；收集器不得漏题）。
- **R6（会话生命周期）**：关闭会话 = 丢弃回合（后端无取消端点，abandoned
  语义由下次 start supersede 收口）；会话内不再调 progress（N/K 读数在
  会话结束回调后由 u2 刷新）。

## Acceptance

- hook 单测状态机：start → 作答 → 判分反馈 → 下一题 → 全卷收集完整
  （漏题 impossible 的断言）；start 失败路径（409/网络）如实落错误态。
- 渲染测试：题面呈现（含 node_name 标注 + difficulty 档位）/ 判分反馈
  两态（对/错）/ 反馈期间作答禁用。
- 持答案红线断言：反馈仅在判分后存在、不跨题泄漏。
- `bun run typecheck` 绿；文案全走 i18n。

## Resolution

（施工后收口）
