---
id: f3
title: 图课程详情页头部常驻占屏 + 检测卡未到期死空间
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

学习图课程详情页（学习 Tab 正文阅读场景）顶部四段常驻不滚没、不折叠，
正文被压到视口下半；知识检测卡在未到期时也整卡常驻。用户原话：「实际
学习而非挑选学习内容时，应该看到的是正文」。怎么收？

## 已核验事实（2026-11-17 对码）

- **头部四段全 `shrink-0` 常驻**（LearningGraphWorkspace.tsx，自然流于
  Tabs 之上）：
  1. 返回行 + 标题/Beta + 目标段（:656-684）——本课 `graph.goal` 与
     title 几乎逐字重复，目标段白占一行（~60px）；
  2. 知识结构/生长控制条（:713-839，含软闸建议 Alert 可再 +~60px）；
  3. 检测语境句（:853-855，`quizGraphEntryContext`「检测与推荐方向相
     反：…」）+ QuizEntryCard（:851-862）；
  4. Tabs 行（:871+）。
  四段合计约 300–420px，`shrink-0` 保证极矮视口下压的是正文区（d1 高链
  注释 :864-870）。
- **检测卡不看 due**：渲染条件仅 `!zeroNodes && detail.course.enabled`
  （:851）；QuizEntryCard 内部 not-due 分支渲染 N/K 读数 +「尚未到期，
  满 6 次学新结算后开放检测」+ disabled 开始检测（QuizEntryCard.tsx:94-104）
  ——未到期时整卡 ≈ 100px 纯死空间，无任何可执行动作。
- **裁决冲突**：u10 R5（2026-11-12）恰恰把 QuizEntryCard **上移到头部区**
  理由是「恒可见、不随滚动没入」（当时发现入口落在滚动流底部看不见）。
  本票实质是修正 u10 R5 的摆位裁决——同一决策人提出，票面留痕，施工前
  需用户确认新摆位。

## Terms（方案草案，施工前过裁决）

- **R-a（头部可折叠）**：结构/生长条 + 标题段合并为可折叠头部——默认
  展开，收起后一行摘要条（如「结构就绪 · 8 主线 · 63 知识组 ▸」+ 展开
  钮），正文区拿到 ~250px+。折叠态 per-course 持久化（localStorage）；
  processing 态（structure/growth 运行中）强制展开（徽章不可被折叠吞）。
  传统课程工作区（CourseWorkspace）头部更矮，可暂不动或同法跟进（施工时
  定）。
- **R-b（检测卡未到期降版）**：not-due 时整卡不渲染，降为语境行内一个小
  入口（如行尾 mini 按钮/图标，悬浮看 N/K 读数）；due 时恢复完整卡。
  due 瞬间从「无感知」到「出卡」的跳变需一条过渡文案（或行内「已就绪」
  标记），避免用户不知道卡去哪了。
- **R-c（目标段去重）**：`graph.goal` 与 title 相同（或极高相似）时不渲
  染目标段；不同才显示。纯呈现层判断，不动数据。
- **R-d（不做）**：不动 d1 内容区高链；不动软闸建议条语义（其本身有
  「选中节点在薄弱集时整条收起」降噪，R-a 的折叠不吞它的 processing 态）。

## 裁决（2026-11-17 grilling，全按推荐收口）

- **Q4 检测卡摆位**：not-due 时语境句 + QuizEntryCard **整段不渲染**；
  due 时整卡自然出现；「已就绪」主信号继续由学习首页横幅承担（u10 R2 点
  行直开检测会话）。**正式替代 u10 R5「入口恒可见」摆位**。传统工作区
  （CourseWorkspace）挂点行为**不变**（Q7 本波不动）。
- **Q5 头部折叠**：标题 + 目标段 + 知识结构/生长控制条（含软闸 Alert）
  收进一行摘要条（如「结构就绪 · 8 主线 · 63 知识组 ▸」+ 展开钮）；
  默认展开、**per-course 持久化**（localStorage——注意 bun test 无
  localStorage，存储访问须可注入/可降级，测试注入 mock）；structure/
  growth 运行中**强制展开**（processing 徽章不可被折叠吞）；返回行与
  Tabs 常驻；检测区（due 时渲染）在折叠区之外，不受折叠影响。
- **Q6 目标段去重**：`graph.goal` 与 title **trim 后逐字相同**不渲染目标
  段；不做相似度阈值。
- **Q7 传统课程工作区本波不动**。

## Acceptance

- 渲染测试：折叠/展开两态 + processing 强制展开；检测卡 due/not-due 两态；
  R-c 去重分支（相同/不同）。
- 双 locale 全部新词条；双主题目验 + `check:theme` + `check:dead-css` 绿。
- 真机目验：学习 Tab 首屏能看到正文（正文标题不需滚动可见）。

## Resolution

已施工收口（2026-11-17，feat/learn1116：`10fee24e8`+审查修复 `e734244b9`/
`c1396bb9c`）。

- **Q4 检测区消隐**：图工作区 due 判定上提（进页+令牌 bump 各拉一次），
  not-due → 语境句+QuizEntryCard 整段不渲染；**拉取失败延时重试恰一次，
  仍失败才落隐藏臂**（C1 审查修复，缩小「实际已 due 却静默消失」窗口）；
  就绪主信号由学习首页横幅承担（u10 R2）。**u10 R5「入口恒可见」摆位正式
  替代**，GLOSSARY「检测就绪」词条同步。CourseWorkspace 挂点行为不变
  （Q7）。
- **Q5 头部折叠**：标题块+目标段+结构/生长条（含软闸 Alert）收进一行摘要
  条（课程名 + structureSummaryText + processing 徽章兜底 + 展开钮）；
  默认展开、per-course 持久化（注入式存取层 `headerCollapse.ts`，损坏/写
  失败降级展开；bun test 无 localStorage 坑已注入 mock 落地）；processing/
  growth 运行中强制展开且钮禁用；返回行/检测区/Tabs 常驻区外，d1 高链未
  动。实现裁量留痕：摘要条含课程名（折叠后保留身份标识）、右列控件随标题
  块收起。
- **Q6 去重**：`graph.goal` 与 title trim 逐字相同不渲染目标段。
- **审查收口**：processing 徽章三元级联两处复现收敛为共享 `processingBadge`
  渲染面（渲染逐字节不变）；QuizEntryCard 外供 progress prop（免同端点双
  请求）creep 接受留痕。
- **验证**：header-collapse 9 例 + quiz-entry 4 例 + soft-gate 回归 9 例 +
  CourseWorkspace 回归 3 例 + C1 重试两臂 fake-timer 用例全绿；
  typecheck/i18n/theme/dead-css 绿；广谱闸绿。
- **真机目验欠账**：学习 Tab 首屏正文可见（正文标题不需滚动）、折叠态跨
  会话持久、双主题目测——Acceptance 真机项留观，目验 FAIL 则重开本票。
