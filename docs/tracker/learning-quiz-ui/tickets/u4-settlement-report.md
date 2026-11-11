# u4: 完成结算页（逐节点对账 + 分级 + 校准裁决）

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（结算呈现）**：u3 会话末题后 `quizComplete(courseId, roundId,
  answers)` → `QuizRoundReport` 呈现页（会话容器内的终态，非独立页面）：
  - 总净入账 `earned_xp`（全卷 Σ，折叠不计薪已扣）；
  - 逐节点对账行 `covered[]`：`node_name` / `answered` / `correct` /
    `earned_xp`——**折叠节点 `earned_xp = null` 呈现「同日已结算 · 不计薪」**
    （learnhub quiz_settled_on_day 语义；不得渲染成 0 或空）；
  - 逐题校准点 `questions[]` 本票不单独呈现（数据已落盘喂校准面，见 R3）。
- **R2（分级文案）**：准确率 = Σcorrect / Σanswered，镜像 settle.rs 分级
  阈值——`≥0.9` excellent / `≥0.6` pass / `≥0.3` half（阈值**注释标注
  settle.rs 常量单源**，前端不自定数值）；三档文案进 u1 词典，呈现带
  简短激励/建议语境句（口语化，遵 o 波口语化基调）。
- **R3（校准裁决——票面裁决项）**：核对 quiz 校准点落盘是否已进
  MemoryHealthPanel 消费的 stats 源 bins（数据源：stats/calibration 聚合
  面 vs quiz 回合落盘）：
  - 已含 → **不接** `GET /api/learning/quiz/calibration`，MemoryHealthPanel
    零改动（本票只留核对记录）；
  - 缺 → 校准 bins 并入 MemoryHealthPanel（呈现形态沿现有 bins，不新造
    面板），端点封装补进 api.ts。
  裁决与证据（查询结果）写进 Resolution。
- **R4（收口链路）**：结算页提供「完成」回到课程详情页；u2 的
  `onQuizFinished` 回调触发 progress 刷新——N/K 归零读数立即可见
  （「距上次检测 0/K」重新积累态）。

## Acceptance

- 渲染测试：报告呈现（总入账 + 逐节点行）/ 折叠态 null 呈现 /
  三档分级文案断言（边界值 0.9/0.6/0.3 恰好命中档）。
- 收口链路断言：完成后 progress 刷新被调用。
- `bun run typecheck` 绿；文案全走 i18n。

## Resolution

已施工收口（2026-11-11，7d3f3f529 @ feat/learn-quiz-ui-u4，基点 d0c399439
= 集成支 feat/learn0928 尖端；双轴评审随 u5）。

- **R3 裁决（票面裁决项）：缺——quiz 校准点未进 stats 源 bins，零面板改动
  零端点接线。**证据链四条：
  1. stats/calibration 聚合面 = `nomifun-learning/src/service/memory.rs`
     `memory_health_stats`（83–246 行），calibration 只从
     `learning_review_log` 聚合（165–176 行 SQL：`rating_source IN
     ('auto','self') AND stability_before IS NOT NULL AND r_pred IS NOT
     NULL`——复习真实推进，诚实过滤）；
  2. `learning_review_log` 全仓 INSERT 仅两处（review.rs:1791 复习作答、
     progress.rs:713 课时推进）——quiz 链零写入（grade 纯读零落账
     routes.rs:945；complete 只落 `learning_xp_events` + 回合行
     quiz.rs:640–651）；
  3. quiz 逐题校准点落盘 = `learning_quiz_rounds.report_json`（quiz.rs
     §校准模块注释 + report_json bind 641 行，q3 形态）；
  4. `quiz_calibration`（quiz.rs:663–734）只回放回合行，独立端点
     `GET /api/learning/quiz/calibration`（routes.rs:168/986），与
     `MemoryHealthStats.calibration` **同构不同源**（quiz.rs:66 自述「#89
     归因读面的 quiz 数据源」——设计上就是两个测量群体：FSRS 推进校准 vs
     quiz 时点校准）。
  **留痕**：缺-分支的「bins 并入 MemoryHealthPanel + 端点封装补进
  api.ts」按票面本应施工，经裁决批复（2026-11-11）留待后续票——与 q3 §7
  后置的 #89 归因读面同属 quiz 记录读面范畴，混源属产品语义决定不在本波
  授权内；api.ts/types.ts 本票禁改维持。u1 api.ts「本波不接」注释与此裁
  决一致，无需回改。
- R1：控制器新增 `report` 相（complete 成功进报告、**onFinished 延后**），
  `QuizRoundReport` 原样入态（折叠节点 `earned_xp=null` 直测钉死：入态与
  finish 回调同引用零归一，绝不渲染 0/空）；`components/QuizSession.tsx`
  导出 `QuizReport`——总净入账（后端 Σ 原样）+ 逐节点对账行
  （node_name/answered/correct/earned_xp）+ 折叠行消费 u1 键
  `quizFoldedNoXp`（整句「同日已结算，本次覆盖保持但不重复计薪」，coverage
  行保持可读）；逐题校准点 `questions[]` 不呈现（渲染断言钉死）。
- R2：准确率 = Σcorrect / Σanswered（`quizReportScore` 跨 covered 聚合），
  阈值常量 `QUIZ_EXCELLENT_RATIO`/`QUIZ_PASS_RATIO`/`QUIZ_HALF_RATIO`
  （0.9/0.6/0.3）组件内注释标注「镜像 settle.rs，单源勿改」；边界值
  0.9/0.6/0.3 恰好命中档直测 + 渲染断言双落。**偏离票面一处（如实呈
  现）**：< 0.3 落第四档 `low`（「需巩固」）——settle.rs else 分支四分之
  一额的如实镜像，不把 quarter 档说成 half（票面只列三档阈值；zh+en 对称
  追加 quizGradeLow/LowHint）。三档标签消费 u1 已备
  quizGradeExcellent/Pass/Half + 追加三档语境句（Hint 键，口语化激励/
  建议）。
- R4：`finish()` 只从 report 相可进（作答/判分/ready 相拒绝零回调），
  completed 相幂等拒绝；「完成」→ `onFinished(report)` → 现有接线清位 +
  bump quizProgressToken（index.tsx 零改动，N/K 归零读数刷新走 u2 既有
  token 驱动重拉）；report 相封口（grade/next/complete/start 全拒绝零迁
  移）；report 相 close 弃报告零回调（已结算回合不回滚，读数待下次进入刷
  新）。
- 词典：zh+en 对称追加 11 键（quizGradeExcellentHint/PassHint/HalfHint、
  quizGradeLow/LowHint、quizReportHeading/TotalXp/Accuracy/RowScore/
  RowXp/Finish，相邻位置与既有 quiz 块同序）；quizFoldedNoXp 首次落消费
  面；gen:i18n +11 键、check:i18n 绿；quizDictionary.test.ts 面清单同步
  扩展 + 报告面占位断言。
- 闸门：`bun run typecheck` 绿；控制器单测（useQuizSession.test.ts）更新
  3 例 + 新增报告相 describe 4 例全绿；渲染测试（QuizSession.render.test
  .tsx）新增 QuizReport 10 例（报告呈现/折叠态/校准点不呈现/en 对称/边界
  值 0.9·0.6·0.3 恰好命中/聚合 0.85 跨档）全绿；learning 目录回归 375/375
  （3 文件 44 例）；check:theme、check:dead-css、check:button-layout-
  contract、check:css-scope 干净。禁改面（index.tsx/QuizEntryCard/
  CourseWorkspace/api.ts/types.ts）零触碰。
