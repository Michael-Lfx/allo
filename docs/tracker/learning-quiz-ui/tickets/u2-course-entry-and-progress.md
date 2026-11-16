# u2: 课程详情页测验入口 + 进度读数

## Status

施工中（2026-11-11 开票）。

## Terms

- **R1（入口区块）**：`CourseWorkspace`（`ui/src/renderer/pages/learning/
  components/CourseWorkspace.tsx`）内新增测验区块，挂点随课程详情页现有
  布局（票面裁决：进度卡内嵌按钮 vs 页面区块，参照 ReviewBanner 与现有
  卡片层次取其一，不新造第三种形态）。区块呈现：
  - 进度读数：`settled_count/trigger_count`（K 单源后端，前端不自定 6），
    learnhub #90 形态——「距上次检测 N/K 次学新结算」；
  - due 态（`due=true`）：「开始测验」按钮可点（接 u3 会话容器，本票先
    留 `onStartQuiz` 回调位，u3 接线）；
  - 未 due 态：按钮禁用呈现、**不隐藏**（与 ReviewBanner 空队列保留入口
    同理——按钮凭空消失违背可预期性）。
- **R2（刷新时机）**：进入课程详情页时 + 测验会话结束回调后刷新
  `quizProgress`；**不轮询**（读数面非门禁，结算后由会话结束回调驱动
  归零呈现；轮询徒增请求面）。
- **R3（409 竞态防御）**：进度刷新竞态（读数非门禁，点开测瞬间可能被
  后端 409 拒）——本票只负责入口区块对 409 的兜底呈现（toast + 刷新
  进度），会话容器内的完整边界态归 u5。
- **R4（课程存在性）**：课程归档（enabled=0）或删除（墓碑）时不渲染区块
  （跟随课程详情页的存在性语义；后端 due_reviews 对归档课程收窄同源）。

## Acceptance

- 渲染测试两态：due（按钮可点 + N/K 读数）/ not-due（按钮禁用 + 读数）；
  归档课程不渲染断言。
- `bun run typecheck` 绿；文案全走 i18n（u1 词典）。
- 真机目验（launch-sim 造数库 8/6 due 态可点）随 u5 走查一并收口。

## Resolution

已施工收口（2026-11-11，5ab1711dc；双轴评审随 u5）。

- **形态裁决（票面授权二选一）**：独立 Card 区块，照课程详情页既有卡片
  层次（recommendedLesson 卡的「左文案右按钮」形态）。「进度卡内嵌」无
  宿主可落——课时进度条已移入左侧大纲栏（CourseWorkspace 内注释可证），
  塞大纲栏即票面禁止的第三种形态。位置固定在推荐课时卡之后：积累循环
  （学新 → 结算）优先、检测次之，且不随 due 摆位（可预期性）。
- R1：`QuizEntryCard`（新组件）= `quizSessionTitle` 块题 + N/K 读数
  （`quizProgressReading`，K 一律取后端 `trigger_count`，前端不自定 6）+
  开测按钮。due 态可点 → `onStartQuiz` 回调位（u3 接线会话容器，集成收口
  提交挂载）；未 due 态按钮禁用呈现、不隐藏 + `quizNotDueHint` 提示（与
  ReviewBanner 空队列保留入口同理）。
- R2：进页挂载拉一次 + `refreshToken` 令牌变化（= index.tsx 的
  `quizProgressToken` 在会话结束回调后 bump）再拉一次；拉取带 last-wins
  序号防旧响应回写；无定时器零轮询。
- R3：`onStartQuiz` 回调拒绝（u3 接线后开测被后端 409 拒的竞态窗口）→
  `errorMessage` 单源映射 conflictBusy toast + 重拉读数（呈现如实归零）；
  会话容器内完整边界态归 u5。
- R4：`course.enabled === false`（归档）不渲染区块也不拉读数；删除课程
  （墓碑）详情页本就不挂载工作区（详情 404 → 列表页），页面级语义覆盖。
- 接线：`index.tsx` 落 `quizSessionCourseId` + `quizProgressToken` 状态位
  + u3 挂载占位注释；**会话容器挂载不做**（集成收口提交接线）。
- 验证：typecheck 绿；check:i18n 绿（**零加键**——u1 词典 16 条 quiz*
  已覆盖本票全部文案面）；`bun run check` 全闸绿；新增渲染/行为测试 11
  用例全绿（`QuizEntryCard.render.test.tsx` 9：due/not-due 两态、en-US
  文案、加载失败降级、刷新时机与不轮询、点击回调、409 toast+重拉；
  `CourseWorkspace.quiz-entry.test.tsx` 2：现存挂块拉读数、归档零渲染
  零拉取）+ learning 目录回归 335 pass / 0 fail。jsdom 宿主跑真实
  effects（CourseWorkspace 经 useSyncExternalStore 无 getServerSnapshot
  不可走 SSR 探针）；`learningApi.quizProgress` 逐用例补丁还原（HTTP
  边界纪律），toast 经 notificationStore 公开快照求差观察。
- 备注：无实质偏离票面之处；测试文件与组件文件为本票新增独占（不与
  u3–u5 文件面相交）。
