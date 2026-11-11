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

已施工收口（2026-11-11，2bb510000 @ feat/learn-quiz-ui-u3，基点 0ec1e2b43；
挂载接线归集成收口，双轴评审随 u5）。

- R1：`hooks/useQuizSession.ts` 导出 `createQuizSessionController`（纯对象
  状态机：`{phase, busy, round, index, answers, feedback, error}`，相 =
  loading/error/answering/graded/ready/completed）+ `useQuizSession` 订阅壳
  （挂载即 start、卸载即 close）；`components/QuizSession.tsx` 导出
  `QuizCard`（显式 props，渲染 seam）+ `QuizSession`（Modal 壳，props
  `{courseId, onFinished(report), onClose}`）。骨架照 ReviewSession 形态，
  不搬 learnhub runner。
- R2：题面全字段——prompt 走 InlineMarkdown（$…$ 出 KaTeX）；`node_name`
  题源标注 Tag；`difficulty` 1–3 呈现、None 不占位（渲染断言双臂）；
  options/matches 原样交 ActivityInput；交错序 UI 不重排。
- R3：提交即 quizGrade → 反馈面板（对/错互斥 + 判底 answer 经 answerText
  按 kind 可读化 + 解析/证据 InlineMarkdown，证据有则呈现）；反馈期作答
  disabled + 提交钮消失 + 下一题推进。
- R4：控制器钉死——graded 相拒绝重复判分（零状态迁移，引用断言）；反馈
  随 next/next/close 丢弃；关闭深等于初始态；作答相渲染零判底泄漏
  （无正确答案/解析/证据字样的渲染断言）。
- R5：response 原样回收（引用相等断言）；`missingAnswerCount` 前置校验 +
  complete 仅 ready 相可进（作答/判分相拒绝零 API 调用），漏题不可能由
  状态机结构保证（next 只从 graded 推进）。
- R6：close 即弃回合（abandoned 由下次 start supersede）；会话内零
  progress 调用（假 api 计数断言）；N/K 读数由集成在 onFinished 后刷新。
- 失败路径：start 409/网络经 errorMessage 单源落 error 相（conflictBusy/
  networkUnstable）+ 重试同入口；grade/complete 失败 toast 后停在原相可
  重交；契约外空回合如实落 error。
- 词典：消费 u1 全部 16 条 quiz* 键 + 复用 reviewSubmitAnswer/
  questionDifficulty/reviewSessionClose；追加 4 条对称键（quizNext/
  quizRetry/quizReadyToSettle/quizCompleteButton，紧邻 quiz 块），gen:i18n
  +4 键、check:i18n 绿。
- 闸门：typecheck 绿；hook 状态机单测 15 例（全旅程/红线门/失败路径）+
  渲染测试 10 例（node_name/difficulty 双臂/反馈两态/反馈期禁用/en 对称）
  全绿；learning 回归 351/351（bun test --isolate）；check:theme、
  check:dead-css 干净。全量套件不相关失败（tauriUpdater/notification
  facade/CreditsContext 等）已对 pristine base 复核为存量/抖动。
