---
id: d2
title: "节标题完整呈现（自绘步骤条替代 arco Steps）+ SVG 图溢出放行"
type: task
status: closed
assignee: zhangyj
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
- 生成侧：课时图提示词三处——`nomifun-ai-agent/src/lesson_content_loop.rs:214`
  （中文可视化条款）、`nomifun-learning/src/generation/lesson.rs:767`
  （VISUAL_STANDARD_BLOCK 英文渲染盘）、`generation/completer.rs:76`（图形修
  复提示）——均只约束 viewBox 存在，未约束内容落在其内。

## 实施要点

- [x] **票内裁决（步骤条）**：自绘——按钮组 `flex-wrap` 折行、完整标题、当前
      步高亮、失败节红、锁定节禁用态（title 原生提示 `practiceGateHint`），
      替换 `Steps` 组件；CSS 覆写 arco Steps 弃选（组件布局不为折行设计）。
      点击切节仍走 `canGoTo`（门禁语义零变化）。
- [x] **票内裁决（SVG 放行方式）**：`ShadowView.tsx` 的
      `.markdown-figure-svg svg` 补 `overflow: visible`（CSS 对 svg 根生效），
      viewBox 外的绘制可见；容器 `overflow-x: auto` 兜横溢。属性注入弃选
      （sanitize 后 svg 形态多样，正则改写脆）。
- [x] 图契约（生成侧三处）：lesson_content_loop 可视化条款补「图形元素与文字
      必须完整落在 viewBox 内——画出画布边缘 = 图形缺陷（渲染端按声明尺寸裁
      剪）」；VISUAL_STANDARD_BLOCK 补 "Every drawn element and label must
      fit inside the viewBox — text or shapes past the canvas edge get
      clipped at render time"；completer.rs 修复提示补 "Keep every drawn
      element and label inside the viewBox"。

## 验收

- `bun run typecheck` 绿（55 错基线逐条一致）；`check:i18n` 绿（步骤条零新增
  文案——复用 sectionStatusFailed/Pending、practiceGateHint、sectionGeneralStep）。
- `cargo test -p nomifun-learning` 全绿（945/0，见 d4 笔）；`cargo check -p
  nomifun-ai-agent` 绿。
- 真机目验（用户侧）：五节步骤条完整可读、可折行、点按切节与门禁行为同旧；
  SVG 图出界文字可见；两主题正常。

## 非目标

- 滚动链（d1）；SVG 内容质量本身（修复按钮已有）。

## Resolution（2026-11-10 施工收口）

**代码面全验收过；真机目验待用户。** 施工 = `c2c7371d5`（前端步骤条 + svg
CSS）+ `69c399e00`（三处生成侧图契约）。

- 自绘步骤条：`flex-wrap` 按钮组（序号圆徽 + 完整标题折行），当前步 primary
  高亮、失败节 danger 浅底、待生成灰、锁定节禁用 + `title` 原生提示；
  `Steps` import 移除。门禁/切节语义与旧实现逐条对齐（canGoTo、上一节随时回）。
- `.markdown-figure-svg svg` 补 `overflow: visible`；三处生成侧提示词补
  in-viewBox 契约（预防同类出界）。
- typecheck 55 错基线逐条 diff 一致（wave 路径零错）；`check:i18n` /
  `check:theme` / `check:css-scope` 绿；`check:dead-css` 学习域零命中。
