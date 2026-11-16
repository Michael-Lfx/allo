# Map: 图课程详情页 UX 修复——已会声明收敛 · 结构图呈现 · 概念表/详情/总览（c1–c8）

<!-- wayfinder:map -->
<!-- 状态：收图（2026-11-09）。c1–c8 全部施工完毕闭票；集成 =
      feat/learning-course-detail-ux，已并回 feat/learn0928。真机 UI 走查（声
      明冻结/勾选保持/自动切 Tab/概念页全链 + 两主题）留用户目验，FAIL 按票
      重开。来源 = 用户四张截图反馈（学习 Tab 声明窗口 / 概念表 Tab / 结构图
      Tab / 概念条目缺失详情页），当批分析后用户拍板立票。建课流程简化（表单
      字段与生成计划页）用户明示「下一批再说」，入后置清单。 -->
<!-- 基线：feat/learn0928 尖端 bffb22b0d；集成分支 = feat/learning-course-detail-ux。 -->

## 用户裁决（2026-11-09，本波次全部工作的依据）

1. **撤销声明只删前端入口**；后端 `DELETE /basis` 接口保留（幂等、有测试锁
   定，留作纠错通道），Rust 面不动。
2. **跨课程概念总览 = 独立页面** `/learn/concepts` + 课程内概念表页头入口；
   概念详情页展示关联关系与学习节点应用。
3. **结构图生成完成后自动切一次 Tab**（structure_completed 首次到达）+ 轻
   提示；之后不自动切换。
4. **勾选状态持久化呈现是第 2 项问题的重点**（用户修正）：声明结果后端有
   持久化（`GET /basis` 锚行投影），但 UI 把已声明门口的 checkbox 替换成小
   tag，草稿勾选又是 GateCoverage 组件本地 state（学习 Tab 与结构图 Tab 两
   个实例不共享），切 Tab/重进后用户「不知道自己选了什么」。修法 = 冻结态
   用 disabled checkbox（勾选态 = 已声明）呈现持久化结果 + 草稿勾选上提共
   享。
5. **已会声明只允许进行一次**，之后冻结为纯展示项（现状可无限轮声明，后端
   幂等 noop 兜底不删）。

## 发现 → 票映射

| 发现 | 归票 |
| --- | --- |
| ① 声明已会按钮 `size='mini'` 藏卡片右下角；用户不知道为什么要声明 | **c1** |
| ② 勾选状态不持久呈现：已声明门 checkbox 被 tag 替换；草稿勾选组件本地、双挂载点不共享；checkbox 在 API resolve 前被清空 | **c1** |
| ③ 声明后反馈只有阻塞 Modal.info + 四处无过滤 WS 静默重拉；声明的调度效果（`detail.graph` 就绪集）从不重拉 | **c2** |
| ④ 撤销声明按钮/确认弹窗整条链（用户裁决：删前端留后端） | **c3** |
| ⑤ 结构图 Tab：过门自检卡夹在视图切换器与画布之间，与四个视图切换不是一组内容 | **c4** |
| ⑥ 结构图生成完成后无任何呈现动作，图藏在 Tab 里 | **c5** |
| ⑦ 概念表看不到概念定义（`definition` 已返回未渲染）；互指只留最后一个 partner（覆盖写 bug）；表无 rowKey；审计失败把整表打错 | **c6** |
| ⑧ 概念无单概念/全局端点（`search_concepts` detail 模式仅 AI 工具无 HTTP 路由）；`other_courses` 只有标题无 id 不能跳转 | **c7** |
| ⑨ 概念无详情页、无跨课程总览入口 | **c8** |

## 影响面关键事实（2026-11-09 实码核实，票面直接引用；开工先复核）

- **GateCoverage 双挂载**：`DeclarationWindow.tsx:77-87`（学习 Tab zero-nodes
  声明窗口）与 `StructureGraphTab.tsx:155-165`（结构图 Tab）都内嵌
  `GateCoverage`，各持独立 checkbox 本地 state（`GateCoverage.tsx:34`）。
- **声明链**：`LearningGraphWorkspace.tsx:273-303` `declareBasis` = POST
  `/basis/declare` → 阻塞 `Modal.info`（:279-289）→ `GET /basis` 重拉；勾选
  在确认 onOk 里立即清空（`GateCoverage.tsx:66`），API 未返回。
- **WS 面**：`useGraphGenerationEvents.ts:5-13` 只过滤 `growth_*` /
  `structure_*`（`detail.graph` 就绪集因此从不因声明刷新）；而 GraphHistory
  （:686-688）、ConceptTable（:880-882）、SystemActionsSection（:791-793）、
  basis 读侧（:264-266）都是无过滤全量订阅——`basis_declared` 一次事件触发
  四处静默重拉。
- **声明后端幂等**（不动）：`declare_basis`（learning_graph.rs:5079-5549）
  重复声明 = noop 零 LLM 零写（:5168-5193，测试
  `basis_declaration_rerun_is_idempotent_noop` 锁定）；撤销 = 全删锚行、概念
  留池（:5628-5667）。冻结态数据源 = `GET /basis`（`BasisView.doors` /
  `declarations`），现成。
- **结构图 Tab 纵向堆叠**：`StructureGraphTab.tsx` 视图切换器（:71-86）→ 挂
  载三态段（:88-153）→ GateCoverage（:155-165）→ 图视图（:167-170）→ 页脚。
- **结构完成监听**：`LearningGraphWorkspace.tsx:145-179` 已订阅
  `structure_completed`（仅 refetch，无导航）；Tab 状态是本地
  `useState('study')`（:87）。
- **概念表**：`LearningGraphWorkspace.tsx:849-1052`。`definition` 每行已返回
  （`GraphConceptRowView`，types.ts:909-924）但 UI 只渲染别名定义（:985）；
  互指 `mutualWith.set` 覆盖写只留最后一个 partner（:897-900）；`<Table>` 无
  `rowKey`（:958-964）；graphConcepts 与 conceptAudit 共享一个 error state
  （:865/:875/:905），审计 409（archived 课程守卫）会打翻整表。
- **概念后端**：registry 全局表无 course/user 列（070 迁移 :182-201）；概念
  ↔节点链接 = `learning_lesson_concepts`（:203-225，concept_id 有索引）；
  `graph_concepts` 的 `other_courses` 只返回标题（learning_graph.rs:4281-4297）；
  `concept_audit` 的自指/互指是纯 registry 子串扫描
  （definition_audit.rs:176-210），可全局复用；`registry_entries_bounded`
  上限 500（:4705-4733）；`search_concepts` detail 模式（:4953-5003）无 HTTP
  路由。
- **路由**：`Router.tsx:414-415` `/learn` 与 `/learn/:id`；静态段
  `/learn/concepts` 优先于 `/learn/:id`，无冲突（先例：`/knowledge/:id`）。
  学习节点选中态是本地 state 无 URL——概念详情页的节点引用 MVP 纯文字展示。
- **已会声明 i18n 面**：zh-CN/en-US `learning.json:499-532` 同 key 同行号；
  删 keys 必须双侧对称（`check:i18n` 只查奇偶）。

## Tickets

| id | 票 | 优先 | 状态 |
| --- | --- | --- | --- |
| c1 | 已会声明一次性冻结 + 勾选状态持久呈现（disabled checkbox 冻结态 + 草稿勾选上提共享 + 按钮升级与动机文案） | 高 | ✅ closed（3d3a5f585） |
| c2 | 声明反馈与数据一致（Message 替代阻塞 Modal + onRefresh + WS 监听收敛） | 高 | ✅ closed（f766ce511） |
| c3 | 删撤销声明（仅前端全链；后端不动） | 高 | ✅ closed（1b5acb939） |
| c4 | 结构图 Tab 布局：过门自检移到图视图之后 | 中 | ✅ closed（ee3fad798；挂载三态段一并下移） |
| c5 | 结构图生成完成自动呈现一次（切 Tab + 轻提示） | 中 | ✅ closed（5aca3276c；在场 + 离场零节点兜底，持久水位） |
| c6 | 概念表修复（definition 渲染 / 互指覆盖写 bug / rowKey / error 拆分） | 高 | ✅ closed（5ad2867aa） |
| c7 | 概念后端端点：单概念详情 + 全局总览 + other_courses 带 id + Rust 测试 | 高 | ✅ closed（7230cb291；learning 944/0） |
| c8 | 概念详情页 + 总览页（路由/两页/概念表入口与行点击；blocked_by c7） | 高 | ✅ closed（383e630f6） |

## 验证总纲

1. 每票：`bun run typecheck`；涉 i18n 先 `bun run gen:i18n` 再 `check:i18n`；
   c7 另跑 `cargo test -p nomifun-learning`。
2. 汇总门：`cargo check --workspace && bun run check`。
3. 真机走查（`bun run dev:web`）：声明一次 → 冻结为勾选态 disabled 展示、
   切 Tab/重进勾选保持、Message 反馈、就绪计数即时更新；结构生成完成 → 自动
   切结构图 Tab 一次；概念表定义可见、互指齐全、行点击进详情、总览入口可达；
   两主题各看一眼。
4. 全过 → map 收图，集成分支并回 feat/learn0928 并推送（合并 main 等用户通
   知）。

## 验证记录（2026-11-09 收图）

- **测试**：nomifun-learning 全量 **944 passed / 0 failed**（含新增
  `concept_view_tests` 3 例）；学习域前端单测 **119 pass / 0 fail**（17 文件，
  含新增 `structurePresentation.test.ts` 9 例）。
- **闸门**：`cargo check --workspace` 绿。`check:i18n` / `check:theme` /
  `check:icons` / `check:button-layout-contract` / `check:css-scope` /
  `check:motion` / `check:error-surface-contract` /
  `check:support-surface-contract` / `check:codemirror-runtime` /
  `check:process-runtime-boundary` / `check:browser-platform-boundary` /
  `check:windows-console-hide` / `check:market` / `check:fingerprint` /
  `check:release-sync` / `help --check` 全绿。
- **存量红（非本波）**：`typecheck` 55 错（videoCanvas 48 等）——c1 与 c8 两次
  全量输出逐条 diff 完全一致，**本波零新增**；`check:dead-css` 红在 Wallpaper /
  Herdsman / CloudLogin / eval 等未触文件，`pages/learning` 零命中。聚合
  `bun run check` 因 `&&` 串联在 typecheck 一步即被存量截断，故逐闸实跑。
- **真实数据冒烟（c7）**：开发库副本起后端被数据集身份守卫拦下（副本的
  work-root 归属标记指向原 Nomi-dev，`factory_reset.rs:969`）——不为冒烟动安全
  层，改用 bun:sqlite 对副本跑端点同款 SQL：登记表 10 概念（全部当前代际、无
  重名）、1 条声明锚覆盖期权课 5 概念（即截图「已会声明」五行）、关系扫描恰 1
  对互指（投入资金 ↔ 百分比损益，与截图审计列一致）、声明概念节点使用 0（对
  应「已会声明（无学习节点）」）。副本已删除。
- **留观**：UI 真机走查（声明一次 → 冻结勾选态 / 切 Tab 与重进勾选保持 / toast
  与就绪计数 / 结构完成自动切 Tab / 概念表行点击 → 详情 → 关系互跳 → 总览）
  留用户在桌面端目验。

## 后置清单（本波不修）

| 项 | 说明 |
| --- | --- |
| 建课流程简化 | 用户明示「下一批再说」：表单只留目标输入、生成计划确认页去留 |
| 概念详情页节点引用可跳转 | 依赖学习节点 URL 化（选中态本地 state），MVP 纯文字 |
| 概念总览分页 | c7 先沿用 registry 上限 500，超限再议分页/检索（c8 已有前端本地筛选 + 50 行分页） |
| 真机冒烟用后端实例 | 开发库原样副本会被数据集身份守卫拒绝启动；下次需要真机后端冒烟时按既有「新建工作目录 + 搬凭据/provider」惯例备阵地 |
