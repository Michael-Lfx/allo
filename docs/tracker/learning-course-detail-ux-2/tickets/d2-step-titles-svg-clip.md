---
id: d2
title: "节标题完整呈现（自绘步骤条替代 arco Steps）+ SVG 图溢出放行"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# d2: 节标题截断 + SVG 内容截断

**What to build:** 两处截断：

1. **分节导航节标题被截断**：arco `Steps size='small'`（LessonStudy.tsx:485-507）
   横排节标题，长标题（「概念：团购券里藏着「锁定买入价」的结构」）窄容器下截
   断不可读。改自绘可折行步骤条（票内裁决，见下）。
2. **SVG 图内容被裁**：模型画的 svg 内容画出其 viewBox/width 声明（截图右卡片
   「发券的店家：…它就得按 15…」在卡片右缘被裁）。渲染层放行溢出 +
   生成侧图契约补一条（票内定）。

## 法证证据（2026-11-10 实码核实）

- Steps：`LessonStudy.tsx:485-507`，title 直接放节标题全文；arco Steps 标题
  默认单行（`white-space` + 固定 item 布局），容器窄时截断；点击切节走
  `onChange` + `canGoTo` 门禁（练习节全部作答才放行，:488-490）。
- SVG：`CodeBlock.tsx:158-164` → `SvgBlock.tsx:118-123`
  （dangerouslySetInnerHTML 注入净化后 svg）；样式
  `ShadowView.tsx:549-553` `.markdown-figure-svg svg { max-width: 100%;
  height: auto }` 只缩放盒子——svg 视口默认裁剪 viewBox 外的绘制内容；
  `.markdown-figure-svg` 容器已有 `overflow-x: auto`（:544）。属性注入先例 =
  `MermaidBlock.tsx:32-41 withResponsiveSvg`（正则改写 `<svg>` 标签属性）。
- 生成侧：课时图提示词在 nomifun-ai-agent `lesson_content_loop`（图形护栏已
  存在）；「图形元素不得画出 viewBox」一条可低成本预防同类。

## 实施要点

- [ ] **票内裁决（步骤条）**：推荐自绘——按钮组 `flex-wrap` 折行、完整标题、
      当前步高亮、失败节红、锁定节禁用态 + 锁提示（复用 `canGoTo` /
      `stepLockedAt`），替换 `Steps` 组件；放弃 CSS 覆写 arco Steps（组件布局
      不为折行设计，覆写脆）。序号保留（1…N）。
- [ ] **票内裁决（SVG 放行方式）**：`ShadowView.tsx` 的
      `.markdown-figure-svg svg` 补 `overflow: visible`（CSS 对 svg 根生效），
      让 viewBox 外的绘制可见；容器已有 overflow-x:auto 兜横溢。不选属性注入
      （sanitize 后的 svg 形态多样，正则改写脆）。
- [ ] 图契约（生成侧，低成本）：在 nomifun-ai-agent 课时图形护栏提示处补一句
      「图形元素与文本必须完整落在 viewBox 内，不得画出画布」（定位到具体
      提示词位置后落点，若护栏形态不允许一句注入则记档后置）。

## 验收

- `bun run typecheck` 绿；`check:i18n`（若新增文案）绿。
- 真机目验（用户侧）：五节步骤条完整可读、可折行、点按切节与门禁行为同旧；
  SVG 右卡片「它就得按 158 元结算」完整可见；两主题正常。

## 非目标

- 滚动链（d1）；SVG 内容质量本身（修复按钮已有）。
