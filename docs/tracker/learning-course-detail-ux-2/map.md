# Map: 图课程详情页 UX 修复二波——滚动/截断/重生成移除/节点命名（d1–d4，持续追加）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-10）。来源 = 用户在真机继续测试的追加反馈（截图：学
      习 Tab 课时内容页）。用户明示「还在测试，可能还有继续追加内容」——本
      波开放追加，新反馈按票补进本目录，不另开波。 -->
<!-- 基线：feat/learn0928 尖端 b29c34a9a；集成分支 = feat/learning-course-detail-ux-2。 -->

## 用户裁决（2026-11-10）

1. **知识结构不再支持重新生成**（可能带来的遗留问题太多）——移除「重新生成」
   入口。结构缺失/失败时的首次「生成」入口保留（否则生成失败后无路可走）；
   后端 regenerate 端点按撤销声明同口径保留（仅撤前端入口），票内可裁。
2. **学习节点命名去模板化**：现网节点名全是「用X认出Y：Z」的类比句式（团购
   券/保价回收/留货优先券/预售锁价/租房押金……全是消费场景类比）。正常课程
   不这么命名；教学意图已有 intent/scope 字段承载（`COACH_SYSTEM` 的节点契约
   本就要求 intent/scope），类比是引入方式不是标题。

## 发现 → 票映射

| 发现 | 归票 |
| --- | --- |
| ① 最严重：学习 Tab 课时内容超视口被静默裁剪，无滚动条，「下一节」按钮不可达 | **d1** |
| ② 分节导航（arco Steps）节标题截断（「概念：团购券里藏着「锁…」）；SVG 图右侧卡片文字被裁（「它就得按 15…」） | **d2** |
| ③ 「重新生成」入口移除（裁决 1） | **d3** |
| ④ 学习节点命名模板化（裁决 2，生长提示词） | **d4** |

## 影响面关键事实（2026-11-10 实码核实，票面直接引用）

- **d1 滚动链根因（arco 默认样式实锤）**：`node_modules/@arco-design/web-react/dist/css/arco.css`
  里 `.arco-tabs-content { overflow: hidden }` +
  `.arco-tabs-content-item { height: 0; overflow: hidden }` +
  `.arco-tabs-content-item-active { height: auto }`。而工作区 Tabs
  （LearningGraphWorkspace.tsx:497）带 `flex min-h-0 flex-1 flex-col
  [&_.arco-tabs-content]:flex-1`——content 被 flex-1 钉在剩余槽高，且
  **overflow 非 visible 的 flex 子项 min-height:auto 退化为 0**（flexbox 规
  范）→ item-active `height:auto`（内容高）溢出 content 槽 → 被
  content 的 overflow:hidden 静默裁掉，全链无滚动条。修法 = 补齐确定高链
  （item-active h-full + overflow-y-auto），各 TabPane 自滚。
- **d1 下一节按钮位置**：节步进条（上一节/N/下一节）在 LessonStudy.tsx:643-663，
  位于整块底部——内容被裁即不可达。
- **d2a 节标题截断**：LessonStudy.tsx:485-507 用 arco `Steps size='small'` 横向
  排节标题（长标题如「概念：团购券里藏着「锁定买入价」的结构」），arco Steps
  标题样式不折行、窄容器截断。自绘可折行步骤条 vs CSS 覆写 → 票内裁决。
- **d2b SVG 截断**：课时图 = CodeBlock.tsx:158-164 → `SvgBlock`（组件
  Markdown/SvgBlock.tsx，dangerouslySetInnerHTML 注入净化后的 svg）。样式
  ShadowView.tsx:549-553 `.markdown-figure-svg svg { max-width: 100%;
  height: auto }` 只缩放盒子——模型画的 svg 内容超出其 viewBox/width 声明
  时，svg 视口默认裁剪（截图右卡片「它就得按 15…」）。MermaidBlock.tsx:32-41
  已有 `withResponsiveSvg` 属性注入先例。
- **d3 重生成入口**：LearningGraphWorkspace.tsx:439-449 摘要栏按钮——label 按
  `structure?.structure` 有无在 重新生成/生成 间切换；`regenerateStructure`
  回调 :181-198；后端 POST `/graph/structure/regenerate` 与 WS `structure_*`
  事件面是首次生成共用的（**不能动事件面**）。i18n
  `learningGraphStructureRegenerate`（zh/en 同段）。
- **d4 命名提示词**：`crates/backend/nomifun-learning/src/learning_graph/mod.rs:494`
  `COACH_SYSTEM`——title 字段说明（:499）只约束「动作句」，未约束类比句式；
  :500 purpose 必须带「日常经验切口」、:528 节点纪律「每个节点的 purpose 必须
  点出从哪段日常经验引入」——类比被强制进 purpose 后泄漏进标题（现网实测全
  批「用X认出Y」）。intent/scope 字段（:501-502）本就是教学意图的正向承载。
  y01 名字软提示通道（mod.rs:440-481 `ng.mints.name_hint`，warn 不阻断）可挂
  句式检测。评审契约 `REVIEW_FIELDS`（growth_contract.rs:47）含 title。
- **d4 内容图 svg 契约**（d2b 的生成侧）：课时图提示词在 nomifun-ai-agent
  lesson_content_loop（图形护栏已存在），补「图形元素不得画出 viewBox」一条
  属低成本预防，随 d2b 票内定。

## Tickets

| id | 票 | 优先 | 状态 |
| --- | --- | --- | --- |
| d1 | 学习 Tab 课时内容滚动缺失——补齐 Tabs 确定高链，各 TabPane 自滚 | **高（最严重）** | ✅ closed（c2c7371d5） |
| d2 | 节标题完整呈现（自绘步骤条）+ SVG 溢出放行 | 高 | ✅ closed（c2c7371d5 + 69c399e00） |
| d3 | 移除知识结构「重新生成」入口（首次生成保留；后端不动） | 中 | ✅ closed（c2c7371d5） |
| d4 | 学习节点命名去模板化（COACH_SYSTEM 标题纪律 + 句式软提示） | 高 | ✅ closed（69c399e00；真机复验待下次生成） |
| d5 | 建课流程简化：学习图只留目标输入，删除「生成计划」页（提交即生成） | 高 | ✅ closed（46125d745；原「下一批」项，用户 11-10 点名开工） |

## 验证总纲

1. 每票：`bun run typecheck`（存量 55 错基线，learning 域零新增）；涉 i18n 先
   `gen:i18n` 后 `check:i18n`；d4 另跑 `cargo test -p nomifun-learning`。
2. 汇总门：`cargo check --workspace`；前端闸（theme/dead-css 学习域零命中等）
   按一 波口径逐闸跑。
3. 真机目验（用户侧，本波触发源就是真机测试）：长课时滚动到底、「下一节」可
   点；步骤条标题完整；SVG 图文字不裁；无「重新生成」按钮；新生成课程的节点
   名不再套类比句式（d4 需一次真机生成才能验）。

## 后置清单（本波不修）

| 项 | 说明 |
| --- | --- |
| （本波持续追加） | 用户仍在测试，新反馈按票补进本目录 |
