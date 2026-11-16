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
- **R5（图工作区入口上移，2026-11-12 桌面走查追加）**：真机实证 u6 的
  「视口底缘常驻」未兑现——入口实际落在滚动流底部，非全屏不可见（用户：
  「需要滚动条到下面，不全屏是无法看到的」）。修法：QuizEntryCard 从学习
  页签两栏区之后**上移到页签头部区**（知识结构标题 + 软闸建议条之下、
  Tabs 内容之上）——该区域不随内容滚动折叠，非全屏恒可见。布局注意：
  LearningGraphWorkspace 有 d1 高链注释（content 高度钉死），头部区为
  无滚动自然流，入口卡置入不影响两栏区滚动语义。
- **R3（回归面）**：横幅渲染测试更新（新文案值 + 行点击断言
  navigate + 置位）；图入口测试更新（新位置：软闸条之下、Tabs 之前）；
  learning 回归绿；typecheck/i18n 绿；theme/dead-css 干净。
- **R4（不做）**：横幅结构不动（聚合/排序/有 due 才渲染语义保留）；CONTEXT.md
  「检测」词条不动（术语不变，只是 UI 措辞）；传统工作区入口位置不动
  （u2 位置在推荐卡之后，页面上部，无折叠问题）。

## Acceptance

- 渲染测试双 locale（新文案值 + 行点击双断言）；全闸绿。
- 真机：学习首页横幅显示「检测已就绪」→ 点行 → 直开检测会话。

## Resolution

已施工收口（2026-11-12，e3a3d64c6 经 merge 合入 learn0928）。

- R1：quizOverviewTitle「检测到期」→「检测已就绪」（Quiz ready）、
  quizOverviewDueTag「已到期」→「已就绪」（Ready），zh/en 对称，键集未变。
- R2：横幅行点击 = 置 quizSessionCourseId + navigate——检测 Modal 经既有
  renderQuizSession 在工作区上立即挂载（图/传统分支共用状态位天然生效）；
  409 走 QuizSession 既有 onConflict。
- R5：QuizEntryCard 上移到页签头部区（结构/生长控制条含软闸条之下、
  Tabs 之前），shrink-0 保留，d1 高链滚动语义不受影响；**补显式
  `!zeroNodes` 门**（零节点声明窗课不渲染头部入口卡——存在性语义等价，
  注释留痕）。
- 验证：learning 回归 408 绿；typecheck/check:i18n/theme/dead-css 绿；
  `bun run check` exit 0（合入后）。测试断言落点：组件层仅 onOpenCourse
  单回调且无 LearningPage 级宿主，行点击断言置位回调被调（course_id
  透传），navigate+置位双动作契约在注释留痕——与 u7 测试架构一致。
- 桌面复验点：学习首页横幅「检测已就绪」→ 点行 → 检测 Modal 直开；
  图课程第一屏（无需滚动/全屏）可见检测入口卡。
