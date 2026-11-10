---
id: c1
title: "学习图建课弹窗最小修——隐藏 v2 步骤条/活动流，running 单行提示，completed 只留卡片"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# c1: 学习图建课弹窗最小修（呈现策略已裁决 = 隐藏 v2 呈现）

**What to build:** `course_kind === 'learning_graph'` 时，`CourseGenerationProgress`
不再渲染 v2 Steps 步骤条与实时活动流：`running` 时渲染单行 spinner +
`learning.graphCreateBackgroundNotice`（新 key，双语：zh「结构生成已在后台启动，
可在课程工作区查看进度」/ en 对应）；`completed` 后只留课程卡片（现状保留）；
`failed`/`cancelled`/`queued` 分支维持既有 Alert/提示（终态以 HTTP 为准的契约
不动）。v2 知识库流/描述流呈现零变化。

## 法证证据（2026-11-10 真机 + 实码）

- 真机（课程 01a1c440，目标「通盘认识股票市场基础」）：提交 → HTTP 秒回 →
  卡片（1 模块 · 0 课时）立即出现；弹窗活动流同屏停留
  「正在生成知识结构与前置地形…」直到结构 15:01 done 之后仍未变——用户看到的
  是 completed 卡片 + 冻结步骤条 + 起始帧文案的分裂态。
- 机制链（map §影响面）：HTTP 秒回（`learning_graph.rs:129-193`）→ status 翻
  completed（`useCourseCreation.ts:101-104`）→ WS 订阅停用
  （`CourseGenerationProgress.tsx:62-67`）→ 步骤条恒 legacy 且 step 恒 1
  （`model.ts:122-146` 只认 round/audit/publishing/completed；graph 流只有
  started 帧；`CreateCourseDialog.tsx:109-117` 不传 `pipeline`）。
- 结构进度的真呈现面已在工作区：`LearningGraphWorkspace.tsx:176-177`
  （structure_started → 徽章，structure_completed → 重拉），e3 声明窗口既有。
  对话框内重复建设已裁决否决。

## 实施要点

- [ ] `CourseGenerationProgress` 增加 `courseKind` prop（或等价的 graph 布尔），
      `CreateCourseDialog.tsx:109-117` 从 `generation.request.course_kind` 传入。
- [ ] graph 分支：Steps + 活动流区域替换为 running 单行提示（含既有取消按钮
      行为不变）；completed/failed/cancelled 终态分支不动。
- [ ] 新 key 双语对称落 `zh-CN`/`en-US` `learning.json`；`bun run gen:i18n` +
      `bun run check:i18n` 过。
- [ ] 顺带订正三处陈旧注释（map §影响面末条）：`api.ts:55-57`、
      `CreateCourseDialog.tsx:14-17`、`useCourseCreation.ts:155-156`——删去
      「首生长在后台启动/执行」（e3 已移除衔接，`learning_graph.rs:189-191`）。
- [ ] v2 回归：描述流/知识库流提交后步骤条、活动流、排队提示行为与改动前
      一致（手工对照一轮即可，无需新测试面）。

## 验收

- 真机：学习图建课一轮——running 单行提示 → completed 卡片；全程无 v2 步骤条、
  无冻结文案；点「开始学习」进工作区，结构徽章/声明窗口正常。
- v2 描述流建课一轮——呈现与改动前一致。
- `bun run check:i18n` + `bun run typecheck` 绿。
- 双主题目验单行提示与卡片。

## 非目标

- 对话框内 structure_* 订阅 / 学习图专用步骤条（裁决否决）。
- 提交后自动跳工作区（裁决否决）。
- 恢复态收敛（c2）、后端文案本地化（c3）。

## Resolution（2026-11-10 施工收口）

**代码面验收全过，已闭票；真机一轮与双主题目验留待用户实测，FAIL 按票重开。**

- 施工 = `1c4e951ea`（图流不渲染 v2 步骤条/活动流、running 单行双语提示
  `learning.graphCreateBackgroundNotice`、三处陈旧注释订正、渲染测试先红后绿）
  + `02402fa55`（票面外必要回归修复：本票让呈现依赖 `request.course_kind`，
  而恢复路径原把所有非生长任务一律标 `learning_graph`——恢复的 v2 建课会被
  渲染成图流单行，违反「v2 呈现零变化」；新增纯函数 `buildRestoredRequest`
  按任务 kind 标注，c2 复用为恢复请求单源）。
- 评审修复（`10b2cb636`，F3）：票面「单行」首版未达（running 两行 spinner、
  queued 同句提示渲染两遍）→ 图流 running/queued 合并为一行（提示 + 取消
  入口）；以修复前组件对照渲染，v2 五种状态与图流 completed 标记逐字节一致。
- 验证（集成树 `56d28dae2`，即本波 + d6–d7 合流后，独立复验）：learning
  156 测试绿；`gen:i18n`/`check:i18n` 绿（11605 keys）；typecheck 54 错 =
  父分支存量（videoCanvas/conversation/credits 等），learning 零新增。
- 留痕：恢复态终态指引文案「可在课程工作区重新发起」对 v2 outline 不完全
  准确（v2 失败时无课程工作区可去），与 c2 后置的 course_id 契约项同源。
