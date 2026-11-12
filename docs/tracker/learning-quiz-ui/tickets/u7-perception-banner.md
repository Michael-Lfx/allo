# u7: 检测感知横幅（首页聚合 + 后端 N/K 列表端点）

## Status

施工中（2026-11-12 开票；grill 裁决 Q3 第一步）。

## 事实源

[分析文档](../assets/2026-11-12-course-kind-quiz-surface-analysis.md) 轴 2
（G3）+ 裁决记录 Q3。问题定性：检测到期零主动呈现（第 6 节点结算那一刻
全应用静默），用户不点按钮 = 功能不存在；learnhub 对照 = 「今日安排」日程
语义，allo 裁决先做首页聚合横幅（日程面不做）。

## Terms

- **R1（后端聚合端点）**：新只读端点（路径票面裁决，推荐
  `GET /api/learning/quiz/overview`）：返回全部启用未删课程中已结算 > 0
  的列表 `[{course_id, title, settled_count, trigger_count, due}]`，due
  优先排序；形态无关（traditional 与 learning_graph 都返回）；无课程返回
  空数组。读数口径与 quiz_progress 单课程端点逐字一致（xp.rs 同源）。
- **R2（首页横幅）**：学习首页（index.tsx）新增检测横幅，语义位紧邻
  ReviewBanner：**有 due 课程才显示**（无 due 不渲染——与 ReviewBanner
  常驻策略刻意不同：复习每天天然有货，检测是稀事件）；每行课程名 +
  N/K 读数 + due 态标记，点课程行直达该课程工作区（传统/图按分支各自
  落到检测入口可见处）。due=false 课程不占横幅（留在课程页内入口）。
- **R3（词典）**：横幅文案 zh/en 双语对称（复用 quiz* 键优先，缺再补）；
  `gen:i18n` + `check:i18n` 过。
- **R4（明确不做）**：浮动坞 Pill（第二步可选，后置留痕）；通知推送；
  「今日安排」面。

## Acceptance

- 端点单测（聚合口径/形态无关/due 排序/空态）；横幅渲染测试（有 due/
  无 due 两态 + 行点击跳转）。
- typecheck + check:i18n 绿；learning 回归绿。
- 真机：seed 造两课程（一 due 一非 due）验呈现与跳转（与 u6 真机走查
  同批执行）。

## Resolution

已施工收口（2026-11-12，1c74262bf）。

- **R1（后端聚合端点）**：`GET /api/learning/quiz/overview`（routes.rs
  挂载紧邻 quiz/calibration）→ `Vec<QuizOverviewCourse>`（course_id /
  title / settled_count / trigger_count / due）。服务实现住 service/xp.rs
  `quiz_overview`：`quiz_settled_nodes` 的 NOT EXISTS 语义原样上移进
  WHERE（`node-completed` 排除其后 `quiz-completed`）后按课程聚合成单条
  SQL（**COUNT(*) 非 DISTINCT**，与 quiz_settled_count 清单 len 同语义）；
  课程筛走「现存且启用」段（deleted_at IS NULL AND enabled = 1，review.rs
  due_reviews 同口径）；due 优先、课程名次之在 Rust 端排序；无符合课程
  返回空数组；形态无关（聚合不读 course_kind）。DTO
  `QuizOverviewCourse` 住 learning models.rs + lib.rs re-export（q1–q3
  存量形态：quiz DTO 实住 learning 而非 api-types，未搬家，前端 types.ts
  按实形镜像）。
- **R2（首页横幅）**：`QuizOverviewBanner`（新组件）——**有 due 课程才
  渲染**：无 due / 拉取中 / 拉取失败整块缺席、零 toast（与 ReviewBanner
  常驻策略刻意不同：复习天然有货、检测是稀事件；课程页内
  QuizEntryCard 入口不依赖本横幅，组件头注释在案）。每行课程名 + N/K
  读数（复用存量 `quizProgressReading` 键，K 一律后端 `trigger_count`）
  + due 标记；行点击 → index.tsx 接线 `navigate(/learn/{course_id})`
  （传统/图工作区检测入口各自可见；图分支入口由 u6 并行落地，本票只管
  跳路由）。数据面组件自拉 `learningApi.quizOverview()`（挂载拉一次，
  last-wins 序号，无定时器零轮询，QuizEntryCard 同款）；挂列表页
  ReviewBanner 之后。
- **R3（词典）**：加 2 键 zh/en 对称——`quizOverviewTitle`（检测到期 /
  Quiz due）、`quizOverviewDueTag`（已到期 / Due）；N/K 读数复用存量
  `quizProgressReading` 零重复；gen:i18n + check:i18n 绿。
- **R4（不做）**：浮动坞 Pill / 通知推送 / 「今日安排」面——维持票面
  裁决，后置留痕。
- 验证：`cargo test -p nomifun-learning --lib quiz` 24 pass / 0 fail——
  新增 `quiz_overview_lists_settled_courses_due_first_and_drops_on_reset`
  （空态；形态无关：traditional 走真实结算管线 + learning_graph 直插
  台账各带结算都返回；due 优先 + 同档课程名排序；N/K 与 quiz_progress
  逐字同源对账；归档 / 墓碑 / 零结算不出列；quiz 结算清零即出列）；
  cargo fmt 过。前端：`bun run typecheck` 绿；check:i18n 绿；check:theme
  / check:dead-css / check:icons 绿；新增
  `QuizOverviewBanner.render.test.tsx` 5 用例（有 due 行呈现与后端序、
  无 due 两形态整块缺席、行点击回调、失败缺席零 toast、en-US 零中文
  回落）+ learning 目录回归 390 pass / 0 fail。真机走查（seed 两课程一
  due 一非 due 验呈现与跳转）留与 u6 seed v3 同批执行。
