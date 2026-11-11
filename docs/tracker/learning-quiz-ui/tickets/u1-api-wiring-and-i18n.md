# u1: API 接线层 + DTO 镜像 + i18n 词典基座

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（DTO 镜像）**：`ui/src/renderer/pages/learning/types.ts` 新增 quiz
  契约类型，字段逐一对齐 `nomifun-learning/src/models.rs`（serde 缺省字段
  在 TS 侧可选项）：
  - `QuizProgressView { settled_count: number; trigger_count: number; due: boolean }`
  - `QuizRoundQuestionView { activity_id, kind: ActivityKind, prompt,
    options: string[], matches: string[], node_id, node_name,
    difficulty?: number }`
  - `QuizRoundView { round_id, course_id, status: 'active'|'completed'|'abandoned',
    settled_count, trigger_count, budget: number, questions: QuizRoundQuestionView[] }`
  - `QuizGradeFeedback { activity_id, correct: boolean, answer: unknown,
    explanation: string, evidence?: string }`
  - `QuizCalibrationPoint { activity_id, correct: boolean, predicted_r?: number }`
  - `QuizRoundNodeReport { node_id, node_name, answered, correct,
    earned_xp: number | null, questions: QuizCalibrationPoint[] }`（null =
    同日折叠不计薪）
  - `QuizRoundReport { round_id, course_id, earned_xp, covered: QuizRoundNodeReport[] }`
  每个类型注释标注单源位置（models.rs 行号或常量名），防漂移。
- **R2（api.ts 封装）**：`learning/api.ts` 按 httpRequest 惯例新增四端点：
  - `quizProgress(courseId)` → GET `/api/learning/courses/{id}/quiz/progress`
  - `quizStart(courseId)` → POST `.../quiz/start`（无请求体）
  - `quizGrade(courseId, roundId, activityId, response)` → POST `.../grade`
  - `quizComplete(courseId, roundId, answers: Record<string, unknown>)` →
    POST `.../complete`
  **不接** `GET /api/learning/quiz/calibration`——校准呈现走 u4 票面裁决
  （stats 源可能已覆盖），接了没人消费的端点违本波节制。
- **R3（i18n 词典基座）**：`zh-CN/learning.json` + `en-US/learning.json`
  双语对称新增 `quiz*` 前缀词条，覆盖本波全部用户可见文案面：进度读数
  （「距上次检测 N/K 次学新结算」形态）、开测按钮、会话标题、题源节点
  标注、判分反馈（对/错/判底/解析/证据标签）、结算分级三档
  （excellent/pass/half）、折叠不计薪、边界态（未 due/缺题/加载失败）。
  键名以实际 UI 用词为准，**先词典后组件**（u2–u4 只消费不新增）。
  `bun run gen:i18n` 更新 `i18n-keys.d.ts` 后 `bun run check:i18n` 绿。

## Acceptance

- `bun run typecheck` 绿；新增类型被 u2–u3 引用无 any。
- 词典双语对称断言（ accompany 单测或 check:i18n 覆盖）。
- 本票无 UI 变更：不渲染、无组件改动，纯接线 + 词典。

## Resolution

已施工收口（2026-11-11，a970df5c38 经 merge 合入 learn0928；双轴评审随 u5）。

- R1：types.ts 七个 DTO 镜像（QuizProgressView/QuizRoundQuestionView/
  QuizRoundView/QuizGradeFeedback/QuizCalibrationPoint/QuizRoundNodeReport/
  QuizRoundReport），注释逐个标注 models.rs 3043–3142 单源行号 + settle.rs
  常量名。
- R2：api.ts quizProgress/quizStart/quizGrade/quizComplete（httpRequest+BASE
  惯例）；**未接** quiz/calibration（节制，注释留 u4 裁决指引）。
- R3：zh/en learning.json 各 16 条 `quiz*` 词条（进度读数/开测/会话标题/
  题源节点标注/判分反馈五面/分级三档/折叠不计薪/边界三态）+ gen 产物
  i18n-keys.d.ts +16 键。
- 验证：typecheck 绿；gen:i18n + check:i18n 绿（12417 keys）；新增
  quizDictionary.test.ts（16 键双语对称/占位逐键对称/键集合一致）4 测试 +
  api.quiz.test.ts（四端点 method/path/body 边界钉死 + 无 quizCalibration）
  6 测试；learning 目录回归 326 pass。TDD 红→绿走通（8 fail → 10/10）。
- 备注：en 侧 quizNotDueHint/quizMissingAnswers 用 em-dash 分隔（zh 逗号
  句式），占位两侧对称已断言钉住；无实质偏离。
