---
id: p1
title: "知识结构可视化原型：dagre + 自绘 SVG 五图裁剪"
type: prototype
status: open
assignee:
blocked_by: [g1]
part_of: map.md
---

## Question

learnhub 已验证的可视化方案（**xyflow 弃用**——边渲染缺陷见其票 #27；dagre 算坐标 + 自绘
SVG 画边 + HTML button 画节点；echarts 仅力导图按需动态 import）移植到 allo（React 19 +
Arco + UnoCSS + 主题/i18n 红线），五图怎么裁剪：

| learnhub 五图 | allo 场景疑点 |
|---|---|
| 思维导图（主干两翼配平） | 与 spines 数据直接对应，疑似必做 |
| 矩阵（主干 × 阶段） | 便宜，疑似必做 |
| 关系图谱（force/layered 双布局 + 枢纽 Top10 + 过滤） | 块图 vs 概念网图，画哪个？ |
| 学习路线（子图 + 传递闭包自动补前导） | 「放弃多终点选择」后参考路线的读侧投影形态 |
| 线路图（地铁） | 可能砍 |

需要原型回答：

1. 首版做哪几张图、各自的节点/边语义（结构块图走 g2 的快照投影；概念网图走
   `learning_lesson_concepts` 投影——两套都要吗）；
2. dagre + 自绘 SVG 在 allo 的 UnoCSS/主题令牌下长什么样（双主题、暗色对比度）；
3. 「投影非权威」标注收敛到详情层的形态；未收录端点虚线菱形占位要不要；
4. blocks 40–80 规模下的交互（缩放/按主干隐藏/按关系过滤）够不够用。

产出：粗糙可反应的原型（独立 HTML artifact 或隔离组件 stub，**不进生产树**——遵守仓库
零 mock 红线），链接回本票供用户反应后再定实施范围。

约束：g1 裁决影响图内容（终点去留）；本票只做原型不做实施。
