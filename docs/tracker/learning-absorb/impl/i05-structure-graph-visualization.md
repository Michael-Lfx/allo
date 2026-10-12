---
id: i05
title: "可视化：工作区「结构图」Tab（三图 + 矩阵）"
type: task
status: open
assignee:
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

- [ ] 四视图 + 小 tab 切换，数据全部来自投影接口（前端不做第二份真相）
- [ ] dagre + 自绘 SVG 落地，blocks 40–80 规模可交互（缩放/按主干隐藏/按关系过滤）
- [ ] 非权威标注三处 + 枢纽琥珀 + 菱形占位 + 过滤器
- [ ] 双主题（check:theme）+ i18n 双语（check:i18n）+ typecheck + dead-css 全绿
- [ ] 课程边界铁律：块图不渲染概念节点、不做块-概念名字匹配
