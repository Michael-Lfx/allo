---
id: i04
title: "前端收尾：向导单步 + 终点/罗盘 UI 退役 + 生成进度衔接"
type: task
status: open
assignee:
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

- [ ] 向导单步（三输入，后两项可选带占位说明）
- [ ] 终点/罗盘 UI 全量退役 + i18n 双语键清理
- [ ] 生成中/完成/失败三态按 structure_* 事件呈现，失败可重试
- [ ] `bun run gen:i18n` + `bun run check:i18n` + `bun run typecheck` 全绿
