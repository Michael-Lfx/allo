---
id: e1
title: SVG 预览固定浅色画布（双主题一致）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

课程正文 ```svg 图形在暗色模式下部分黑字看不清（截图：标题「两条依据
→ 两个区域」、底部「判断只看两件事…」深灰字在深底上几乎不可见；浅色
盒内文字可读）。根因与修法？

## 已核验事实（2026-11-18 对码）

- 渲染端：`.markdown-figure-svg { background: var(--bg-1); color:
  var(--text-primary) }`（ShadowView.tsx :548-556 邻域）——暗色模式画
  布 = 深底、继承文字 = 浅色。
- 模型作画默认**白画布假设**：显式深色文字 fill（标题/旁注）+ 浅色盒
  fill（盒内文字可读纯粹因为盒自带浅底）。生成契约
  （lesson_content_loop.rs:214）无画布背景/配色条款。
- 推论：任何「画布上裸文字/裸图形（无自带浅底）」的图形在暗色下都不可
  读；带自绘深底的图形在白画布上仍自洽（其文字浅色在自己的深底上）。

## Terms（方案草案，施工前过裁决）

- **R-a（荐，主修）渲染端固定浅色画布**：`.markdown-figure-svg` 背景
  固定白色（如 `#ffffff`）、继承 `color` 固定深色（如 `#24292f`）——
  双主题一致，**存量课时立即修复**，零内容迁移。注释写明「模型图形按
  白画布作画，画布是图形的一部分而非主题面」（非 token 面的显式偏差，
  红线留痕）。
- **R-b（辅）生成契约补背景条款**：lesson_content_loop.rs:214 契约追加
  「图形必须自绘背景 rect（浅色，如 #ffffff）覆盖整个 viewBox」——新
  内容自洽（即使渲染端画布策略变也不裸奔）。只影响新内容，不回填存量。
- **R-c 不做**：CSS invert/主题化改写 SVG fill（脆弱且破坏作者意图）；
  mermaid/jsxgraph 主题面不动。

## Acceptance

- 暗色主题下截图对比：标题/旁注/底部句可读（真机目验）。
- 浅色主题无回归（白底卡观感正常）；theme / dead-css 闸绿。
- R-b 若上：契约文本变更带测试（契约是渲染函数生成的文本，现有契约测
  试跟进）。

## Resolution

（未结案）
