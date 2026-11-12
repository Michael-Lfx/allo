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

（施工后收口）
