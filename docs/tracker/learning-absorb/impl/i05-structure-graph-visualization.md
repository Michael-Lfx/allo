---
id: i05
title: "可视化：工作区「结构图」Tab（三图 + 矩阵）"
type: task
status: closed
assignee: zhangyj
blocked_by: [i03, i04]
part_of: ../map.md
---

# i05: 可视化：工作区「结构图」Tab（三图 + 矩阵）

**What to build:** 工作区新增「结构图」Tab（内部小 tab 切换四视图）：思维导图（主干两翼
配平总览）、关系图谱（dagre LR 分层，**边方向反转：assumes = 门**）、学习路线（选主干
子图 + 传递闭包自动补前导，虚线框「前导」角标）、矩阵（主干×阶段，顺带）。技术栈 dagre
算坐标 + 自绘 SVG 连线 + HTML button 节点（xyflow 弃用，不引入；v1 不引 echarts）。机械
采纳 learnhub 已验证形态：详情层非权威标注（「由课程内容自动整理，仅供参考；学习顺序以
教练实际安排为准」）、未收录端点虚线菱形占位（◇ 前缀）、枢纽 Top10 琥珀描边、按主干
隐藏、按关系过滤、页脚「投影非权威；层号=依赖深度，不是排期」。双主题语义令牌 +
i18n 双语红线；数据源 = i03 投影接口。

**Blocked by:** [i03 投影 API](i03-structure-projection-api.md)、
[i04 前端收尾](i04-wizard-single-step-ui-retirement.md)（Tab 宿主顶条清场后落图）。

**Status:** ready-for-agent

- [x] 四视图 + 小 tab 切换，数据全部来自投影接口（前端不做第二份真相）
- [x] dagre + 自绘 SVG 落地，blocks 40–80 规模可交互（缩放/按主干隐藏/按关系过滤）
- [x] 非权威标注三处 + 枢纽琥珀 + 菱形占位 + 过滤器
- [x] 双主题（check:theme）+ i18n 双语（check:i18n）+ typecheck + dead-css 全绿
- [x] 课程边界铁律：块图不渲染概念节点、不做块-概念名字匹配

## Resolution

**速览**：全项落地（feat/learn0928，本票单提交）。

- 新增 `components/structure/` 七件：`palette`（主干固定色板 + 边型配色 +
  枢纽色）、`Mindmap`（两翼配平，块数降序分翼/翼内保序）、`RelationNet`
  （dagre LR 分层 + 自绘 SVG + HTML button 节点；**assumes 门方向**——布局
  前置排左、箭头前置→需求方，composes 方向保持、contrasts 虚线无箭头）、
  `RouteMap`（选主干 = 子图 + 传递闭包自动补前导，虚线框「前导」角标 +
  已访问集防环）、`MatrixView`（主干×阶段表）、`BlockDetail`（五键结构位 +
  依赖层 + 双向关系 + 固定非权威文案）、`StructureGraphTab`（容器 + 页脚
  「投影非权威；层号 = 依赖深度，不是排期」）。
- 工作区第 4 个 Tab「结构图」（i04 已挂状态条；Tab 直接消费 workspace 的
  structure 投影状态，未生成/生成中转 Empty 态）。
- p1 机械采纳全落：详情层非权威标注、枢纽 Top10 琥珀描边 + 度数和、
  按主干隐藏 chips（点击切换）、按关系过滤四值、◇ 菱形虚线占位（防御渲染；
  契约保证端点解析域 = 块表，正常不出现）、页脚硬标注。
- 红线核查：`@dagrejs/dagre` 本已在 ui/package.json（无新依赖）；双主题
  只走语义令牌（节点底/文字/边框用 var(--color-*)，固定色板仅为主干身份
  数据语义）；dead-css 首次跑出 9 处 `border-{t,b,r}-1` 死类——已按仓库
  方向边框惯用法（`border-b border-b-solid border-b-[…]`）修正归零；
  i18n 33 键双语对称（11296 键，check:i18n 过）；学习页 typecheck 零错误；
  check:theme 通过。
- 口径注记：枢纽度数展示为 assumes+composes 出入度和（与后端投影 hubs
  同口径）；矩阵列头阶段词表在 i18n（入门/进阶/高级 ↔ Entry/Intermediate/
  Advanced），映射按后端 STAGES 固定三词。
