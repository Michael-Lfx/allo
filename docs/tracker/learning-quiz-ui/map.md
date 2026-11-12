# Map: quiz 前端面——测验从 API 就绪到可点击

<!-- wayfinder:map -->
<!-- 状态：**已到达目的地（2026-11-11）**——u1–u5 全关（u1 a970df5c38 /
      u2 5ab1711dc / u3 2bb510000 / u4 7d3f3f529 / u5 修复 e7ed96692+
      f0a1aea75，交付合并 4ee392c9c 已推）。双轴评审 4 处必修单代理收口；
      红线全闸 exit 0；learning 回归 385 绿。真机走查 T1 通过（入口卡
      due 态 DOM+截图双证据），T2–T5 点击级因 IAB webview 宿主故障受阻
      如实留痕（u5 Resolution），桌面点击走查留用户（造数态 8/6 未消费）。 -->
<!-- 来源：上线前裁决——测验后端全链就绪（learning-quiz 波 q1–q3：条款→触发→
      载体→质量）而 SPA 零 quiz 面（ui/src 零引用、词典零词条）。launch-sim
      造数波（2026-11-11）把测验验到 API 层为止；本波补前端，让测验成为
      可点击的用户旅程。 -->
<!-- 参考实码：learnhub `C:\Users\test\Desktop\my\learnhub\src\pages\learn\ui\`
      （learn-page/sections/session-runner/start-screen——teach/review/quiz/
      practice 统一会话运行器）与 `src\pages\progress\ui\progress-page.tsx`
      （quizHistory/quizRecordDetail/校准面板）。注意架构差异：learnhub 是
      统一会话运行器，allo 是专用组件架构（ReviewSession 模式）——本波照
      allo 自己的骨架建，不搬 runner。铁律：先读参考实码再动 allo。 -->

## Destination

学习课程详情页出现测验入口：due 态可点开测 → 跨节点交错答题（逐题确定性
判分 + 判底/解析/证据揭示）→ 全卷结算呈现（逐节点对账 + 净入账 + 分级）→
N/K 归零回到积累态。九题型作答控件复用复习会话的 ActivityInput；i18n/
主题/布局红线全过；真机走查用 launch-sim 造数库一键喂 due。

## Notes

- **后端契约事实源**（`nomifun-learning/src/routes.rs` + `models.rs`，
  lib.rs re-export；DTO 实住 learning models 而非 api-types 是 q2/q3 存量
  形态，前端按实形镜像，搬家不做）：
  - `GET  /api/learning/courses/{id}/quiz/progress` → `QuizProgressView
    {settled_count, trigger_count, due}`（读数非门禁）；
  - `POST /api/learning/courses/{id}/quiz/start` → `QuizRoundView
    {round_id, course_id, status, settled_count, trigger_count, budget,
    questions[]}`；**无请求体**；start 慢路径 = 变式链 LLM 调用（有凭据时
    内联生成，VARIANT_V2 证据门；零凭据诚实跳变式只出 bank 旧题）；
  - `POST /api/learning/courses/{id}/quiz/rounds/{round_id}/grade` 请求
    `{activity_id, response}` → `QuizGradeFeedback {activity_id, correct,
    answer, explanation, evidence?}`（**纯读零落账**）；
  - `POST /api/learning/courses/{id}/quiz/rounds/{round_id}/complete` 请求
    `{answers: {activity_id: response}}`（**必作答，缺题拒绝**）→
    `QuizRoundReport {round_id, course_id, earned_xp, covered[]}`；
  - `GET  /api/learning/quiz/calibration` → `Vec<MemoryCalibrationBin>`。
- **题源红线（持答案）**：`QuizRoundQuestionView {activity_id, kind, prompt,
  options, matches, node_id, node_name, difficulty?}` **判底零字段**——答案
  与解析只随 GradeFeedback 判分后下发；UI 不得缓存或提前展示。题面渲染走
  InlineMarkdown 管线（题面契约允许 $…$ 与内联 markdown，复习会话同款）。
- **对账与折叠**：`QuizRoundNodeReport {node_id, node_name, answered,
  correct, earned_xp: number|null, questions: QuizCalibrationPoint[]}`——
  `earned_xp = null` = 同日已结算折叠（coverage 保持、不重复计薪）。
- **常量（settle.rs 单源）**：K = QUIZ_TRIGGER_SETTLEMENTS = 6；题量预算
  QUIZ_QUESTION_BUDGET = 15；分级 QUIZ_EXCELLENT_RATIO 0.9 / PASS 0.6 /
  HALF 0.3。前端只镜像呈现，不得自定阈值。
- **触发口径**：`node-completed` 事件数（锚 `node:{lesson_id}` 终身一笔，
  上次 quiz-completed 后才计数）；结算即清零重新积累。
- **复用面**：`ActivityInput`（九题型作答控件，复习会话同款，ordering/
  matching/multi_choice/numeric/fill_in_blank/true_false/single_choice 全
  支持；reflection/open_question 不会出现在 quiz 题源——题源 = 已结算节点
  客观题 + 变式）；`ReviewSession`/`useReviewSession` 是会话容器骨架范本；
  `MemoryHealthPanel` 已消费 stats 源校准 bins。
- **入口裁决（map 级）**：v1 入口放课程详情页（CourseWorkspace）——后端
  progress 是 per-course 查询，天然契合；学习页跨课程聚合横幅**后置**（需
  新后端聚合端点：每课程 N/K 列表，本波不做）。
- **验证基建**：launch-sim 波 `scripts/seed-learning-triggers.mjs` 一键喂
  due（8/6）+ 真机走查清单——本波真机验收直接复用。
- **纪律**：一票一会话；Resolution 收口 + Conventional Commits；i18n 双语
  对称（gen:i18n → check:i18n）；主题双态（check:theme）；dead-css 零警告；
  双轴评审。

## Decisions so far

- **u2 已裁**：due 态入口 = 独立 Card 区块（照 recommendedLesson 卡「左文案
  右按钮」既有形态），固定在推荐课时卡之后不随 due 摆位；「进度卡内嵌」无
  宿主可落（课时进度条已移入左侧大纲栏）。
- **u4 已裁（记录制）**：quiz 校准点**未进** stats 源 bins——stats/calibration
  只聚合 learning_review_log（memory.rs:165-176），quiz 链零写入；校准点落
  `learning_quiz_rounds.report_json`，quiz_calibration 独立端点回放回合行
  （#89 归因读面设计态）。零面板改动零端点接线；**后置**：quiz 校准呈现
  并入面板留待后续票（与 #89 归因读面同属 quiz 记录读面范畴）。
- **u4 补裁**：分级四档而非三档——`<0.3` 如实镜像 settle.rs else 分支为
  low「需巩固」（票面原列三档系笔漏）。

## 后置（本波不做，留痕）

- 学习页跨课程聚合测验横幅：需新后端端点（每课程 N/K 列表）。
- quiz 校准呈现并入 MemoryHealthPanel：与 #89 归因读面同范畴（见 u4 Resolution）。
- **节点级复习/补习 = 整套缺（2026-11-12 勘察裁决：暂不处理）**——调度系统
  唯一粒度是题卡（FSRS）；「重教」仅存在于课程生长期的螺旋结构语义
  （learning_graph，面向未来学习者）；lesson_progress 单向到 completed 无
  重学态；quiz 测出弱节点零引回正文机制。与之相对，**题卡级反哺（g4）是
  「原语齐、只缺链路」**：建卡 ensure_review_item / 推进 update_mastery_
  and_review / 落库加题 create_lesson_activity 三原语俱在（q3 §7 在案），
  可独立立票——两件事不要混为一谈。

## Tickets

| 票 | 标题 | 依赖 |
|---|---|---|
| [u1](tickets/u1-api-wiring-and-i18n.md) | API 接线层 + DTO 镜像 + i18n 词典基座 | 无 |
| [u2](tickets/u2-course-entry-and-progress.md) | 课程详情页测验入口 + 进度读数 | u1 |
| [u3](tickets/u3-quiz-session-container.md) | 测验会话容器（开测→逐题判分→全卷收集） | u1 |
| [u4](tickets/u4-settlement-report.md) | 完成结算页（逐节点对账 + 分级 + 校准裁决） | u3 |
| [u5](tickets/u5-edges-and-redlines.md) | 边界态 + 红线全闸 + 双轴评审 + 真机走查 | u2, u3, u4 |
