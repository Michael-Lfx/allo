---
id: o6
title: "呈现密度与信息架构——顶条三态合一、双挂载去重、首屏精简（施工最后）"
type: task
status: closed
assignee:
blocked_by: [o1, o2, o3, o4, o5]
part_of: ../map.md
---

# o6: 呈现密度与信息架构（探查「一屏多提示」重灾区 top 5）

**What to build:** 五个重灾区的呈现收敛（方向性设计，施工时以 v 波并回后
现状为准，行号需复核）：

1. **工作区知识结构顶条三 Tag 并排**（`LearningGraphWorkspace.tsx:549-641`）：
   「生成中/自动准备中/生长中」多个 processing Tag 可同时亮——合并为**单一
   状态徽章**（优先级：结构生成 > 生长 > 自动准备；v2 的实时读数徽章作为
   结构态的呈现载体），一行只讲一件事，其余转次要行。
2. **首页顶区堆叠**（`index.tsx:284-331`）：永久 `packContract` 架构词
   info Alert **移除**（o1 后 error 已走友好 toast；该 Alert 对用户零价值，
   内容若需保留移入开发者文档/关于页）；error Alert 改 toast；ReviewBanner
   内部折叠条保持但与顶区间距收紧。
3. **GateCoverage 卡双挂载去重**：`LearningGraphWorkspace.tsx:661-677` 与
   `StructureGraphTab.tsx:167-177` 各挂一份——收敛为单一挂载点（学习 Tab
   为主，结构图 Tab 以摘要/入口替代整卡复渲染）。
4. **StructureGraphTab 挂载/修补条**（`:80-163`）：挂载计数 + stale 警示 +
   挂载索引逐行 + observations Tag 列表平铺——observations 与索引收进
   Collapse，顶条只留结论（含 o5 的 `learningGraphMountCounts` 新口径
   「已对应 N 个，待检查 M 个」）。
5. **建课完成视图**（`CourseGenerationProgress.tsx:385-469` +
   `CourseSourceCoverageCard.tsx:60-207`）：默认只留结论句（标题/计数/
   采样一行/结构态行），警告行、fallback、重复、5 组 Collapse 明细全部
   收进折叠；失败视图 Alert 内嵌错误长文改为摘要句 +「复制详情」按钮
   （与 o1 的原文收纳一致）。

## 法证证据（2026-11-11 组件面探查五、条 26–30）

- 用户点名的「三个 processing Tag 并排」即条 26；GateCoverage 卡一行内
  计数 Tag + hint + 动机 Alert + 每行 4 Tag + 底部 2 Tag（条 28）。
- `packContract`（「课程包由 Course、Module、Lesson…组成」）是架构词汇
  常驻学习者首屏（词典面 J1 #1 同判）。

## 实施要点

- [ ] 先做 o1–o5（文案定稿后再收布局，避免二次改写）。
- [ ] 每个重灾区改前截 v 波并回后现状（真机截图或渲染探针），改后同法
      对照——密度可比较（Tag/Alert/折叠组数量 before/after）。
- [ ] 双主题目验必做（本票是布局级改动，红线 3 适用）。
- [ ] 既有渲染测试同步；不新增轮询面（v2/v3 的轮询源复用）。

## 验收

- 五个重灾区 before/after 对照：同屏提示元素数量显著下降（票面施工时
  记录数字）；无信息丢失（折叠可展开）。
- 双主题目验 + typecheck / check:i18n / check:theme / learning 测试全绿。

## 非目标

- 新增引导/图例功能（口语化后仍需解释的点先记录后置）。
- v2/v3 已落地的徽章/Pill 真话面回改。
- 学习模块之外的布局。

## Resolution（2026-11-11 施工收口）

**代码面验收全过，已闭票；双主题真机目验与欠账合并执行，FAIL 按票重开。**

- 施工 = `175b8ec9b` + `59f3151da`（learning 页面 + 双语词典 + i18n-keys.d.ts；票外顺手改零）。五重灾区 before/after 密度读数（评审指出票文件缺记录，补录于此）：
  ①工作区顶条 processing Tag 最大同屏 **3→1**（结构读数徽章优先级合并；被压态转次要行）；②首页顶区常驻 Alert **2→0**（error→非阻塞 toast；packContract 双语删键，删前 grep 确认唯一使用点）+ 列间距 20→16px；③GateCoverage 整卡渲染 **2→1**（学习 Tab 声明窗口保整卡，结构图 Tab 收为默认收起摘要入口）；④挂载条顶条外默认可见元素 **stale 行+N 索引+≤4 观察 Tag+1 内层折叠 → 1**（单一「详情」折叠，o5 计数口径与 o4 观察摘要原样保留在内）；⑤完成视图默认 **≤7 行 → 2 行**（主题覆盖+去向）+ 1「详情」折叠；失败视图原文常显滚动块 → 摘要句 +「复制详情」（复用仓内 copyText，无新剪贴板基建）+ 折叠「错误详情」。
- 布局裁量 7 条（留痕，评审核对）：次要行 `·` 连接、结构运行中加载失败句降级为次要行、stale 指引并入详情、GateCoverage 摘要入口默认收起（含已声明读数）、来源覆盖卡中性例外行也入详情、列间距最小实现、空折叠壳不再渲染。
- 评审修复 = `276ca3b78`（F7a 顶条三枚 processing Tag 提 ProcessingStateTag 组件——v2 徽章数据源原样；F7b GrowthTaskPill 单 progressNode）。
- 新键 4 个双语对称 + 真实词典 JSON 断言（presentationDensity.test.ts）；`check:theme` 6 主题过；306 绿（施工时）→ 316（全波终态）绿。
