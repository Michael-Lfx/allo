---
id: i04
title: "前端收尾：向导单步 + 终点/罗盘 UI 退役 + 生成进度衔接"
type: task
status: closed
assignee: zhangyj
blocked_by: [i02, i03]
part_of: ../map.md
---

# i04: 前端收尾：向导单步 + 终点/罗盘 UI 退役 + 生成进度衔接

**What to build:** 学习图建课向导退化为单步：输入学习目标（必填）+ 程度声明（可选）+
基础声明（可选）→ 提交即建课并生成结构。删除：第 2 步多终点选择（graphStep 状态机、
draftEndpoints、终点标题/程度声明输入、propose 调用）、终点 chips 与编辑弹窗、罗盘卡与
手动重画按钮、对应 i18n 键（zh-CN/en-US 对称，gen:i18n + check:i18n）。生成进度按 WS
`structure_started/completed/failed` 事件展示（生成中态），完成后经投影接口取数刷新
（雾区「生成进度与可视化衔接」在此收口：宿主 = 工作区「结构图」Tab，i05 落图）。

**Blocked by:** [i02 结构生成管线](i02-structure-generation-pipeline.md)（单步要真的能
生成）、[i03 投影 API](i03-structure-projection-api.md)（完成态取数）。

**Status:** ready-for-agent

- [x] 向导单步（三输入，后两项可选带占位说明）
- [x] 终点/罗盘 UI 全量退役 + i18n 双语键清理
- [x] 生成中/完成/失败三态按 structure_* 事件呈现，失败可重试
- [x] `bun run gen:i18n` + `bun run check:i18n` + `bun run typecheck` 全绿

## Resolution

**速览**：全项落地，commit `4dca5dc5b`（feat/learn0928）。

- **向导单步**：CreateCourseDialog 学习图 tab 收敛为三输入——学习目标（必填）
  + 程度声明（可选，留空由结构生成扩写）+ 基础声明（可选，留空纯零基础）；
  第 2 步终点确认（graphStep 状态机、draftEndpoints、提议调用、两步 footer/
  cancel 语义）整体删除。`useCourseCreation` 新增 `graphProficiency/graphBasis`
  状态，`confirmGraphCreation` 直提三项输入。
- **UI 退役**：工作区终点 chips、编辑弹窗（含完成开关）、CompassCard 删除；
  `learningApi` 的 propose/终点 CRUD/罗盘重画五方法与 `ProposeEndpointsRequest`/
  `ProposedEndpointView`/`GraphEndpointView`/`EndpointInput`/`EndpointUpdateInput`
  五类型删除；`LearningGraphView` 类型去掉 compass/compass_updated_at/endpoints
  （后端 i01 已不序列化——此前运行时取 `graph.endpoints.map` 会崩，本票修复）；
  `GenerateCourseRequest` 增 proficiency_statement/learner_basis。
- **生成进度衔接（雾区收口）**：宿主 = 工作区顶条「知识结构」状态条——由 WS
  `structure_started/completed/failed` 驱动三态（生成中 chip / 已生成摘要
  「N 主干 · M 块 · K 门口」+ 时间 + 重新生成 / 未生成 + 生成知识结构按钮），
  失败经 regenerate 端点可重试；WS 订阅清单加入 structure_*、移除 compass_updated。
  可视化的完整落点在 i05 的结构图 Tab。
- **i18n**：终点/罗盘/提议 20 键删除、程度/基础/结构 13 键双语对称新增；
  `gen:i18n` 重生成（11265 键）+ `check:i18n` 通过；学习页 typecheck 零错误
  （聚合 typecheck 仍 exit 2，54 个错误全为 videoCanvas/videoGeneration 存量
  基线，与学习模块无关）；dead-css 失败文件清单零学习页（存量同样无关）。
