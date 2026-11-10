---
id: d1
title: "学习 Tab 课时内容滚动缺失——arco Tabs 裁剪链（内容被静默裁剪、下一节按钮不可达）"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# d1: 学习 Tab 课时内容滚动缺失（最严重）

**What to build:** 学习 Tab 的课时内容超视口时被静默裁剪——无滚动条、节步进条
（上一节/N/下一节，位于整块底部）不可达。修法 = 补齐 Tabs 的确定高链，让每个
TabPane 在固定内容视口内自滚；学习 Tab 两栏（导航 aside + 课时内容 section）
在链内各自滚。

## 法证证据（2026-11-10 实码 + arco 默认样式实锤）

- arco 默认样式（`node_modules/@arco-design/web-react/dist/css/arco.css`）：
  - `.arco-tabs-content { width: 100%; overflow: hidden; padding-top: 16px }`
  - `.arco-tabs-content-item { width: 100%; overflow: hidden; height: 0 }`
  - `.arco-tabs-content-item-active { height: auto }`
- 工作区 Tabs（`LearningGraphWorkspace.tsx:497`）：
  `flex min-h-0 flex-1 flex-col [&_.arco-tabs-content]:flex-1`——工作区根
  `app-page-shell h-full overflow-y-auto` → 内容列 `h-full flex-col` → Tabs
  `min-h-0 flex-1` 被钉在剩余槽高 → `.arco-tabs-content` 是 flex 子项 flex-1，
  但 **overflow 非 visible 的盒子的 automatic minimum size 退化为 0**（CSS
  flexbox 规范）→ content 高度钉死在槽高；`content-item-active height:auto`
  = 内容自然高 → 溢出 content 盒 → 被 `.arco-tabs-content` 的
  `overflow: hidden` **静默裁掉**。学习 Tab 两栏行的 `min-h-0` 无 h-full 支撑，
  行高 = item 内容高，section 内层 `h-full overflow-y-auto`（:585）的 h-full
  百分比失去确定参照 → 内部滚动永不生效。全链无滚动条，与截图一致。
- 「下一节」按钮在 LessonStudy.tsx:643-663——整块底部，内容被裁即不可达。
- 其余三个 Tab 受同一链影响：GraphHistory 自带 `overflow-y-auto`（本波后其内
  层滚轮照常）；ConceptTable / StructureGraphTab 自然流高——补链后**必须**有
  item 级滚动兜底，否则表格/结构图也会被裁（修复方案必须一并覆盖）。

## 实施要点

- [x] Tabs className 补：`[&_.arco-tabs-content]:min-h-0`（保险）+
      `[&_.arco-tabs-content-item-active]:h-full` +
      `[&_.arco-tabs-content-item-active]:overflow-y-auto`——item 钉在 content
      槽高并自滚；arco 的 content-inner 高度过渡动画对该面板失效可接受（工作
      台不依赖）。
- [x] 学习 Tab 两栏行（:514）补 `h-full`：aside（:515）改
      `max-h-full overflow-y-auto`（节点多时自滚）；section 已
      `flex-1 overflow-hidden`，内层 `h-full overflow-y-auto` 随链生效（滚动
      条回到内容区）。
- [x] 零节点声明窗口分支（:500-512）：内容短自然流，钉高后无回归。
- [x] 四 Tab 自查：history / concepts / structure 均靠 item 级滚动兜底（统一
      钉高 + item 自滚）；概念表 `scroll={{ x: true }}` 横滚与纵向 item 滚共存
      （overflow-y-auto 只接管纵向）。

## 验收

- `bun run typecheck` 绿（55 错基线逐条一致，wave 路径零错误）。
- 真机目验（用户侧）：长课时滚动到底、「下一节」可达可点；学习记录/概念表/
  结构图三 Tab 在内容超视口时出现滚动条；切 Tab 无布局跳变。

## 非目标

- 节标题截断与 SVG 截断（d2）；arco 组件面全局覆写（只动本工作区实例）。

## Resolution（2026-11-10 施工收口）

**代码面全验收过；真机目验待用户。** 施工 = `c2c7371d5`（与 d2/d3 同笔，文件
重叠）。

- Tabs 任意变量类补 `[&_.arco-tabs-content]:min-h-0` +
  `[&_.arco-tabs-content-item-active]:h-full` +
  `[&_.arco-tabs-content-item-active]:overflow-y-auto`；学习 Tab 两栏行补
  `h-full`，aside 改 `max-h-full overflow-y-auto`。四个 Tab 在固定内容视口内
  各自滚动（学习 Tab 内容区/导航列内滚，其余三 Tab item 级滚）。
- 真机判据（长课时滚到底/下一节可点/三 Tab 滚动条）留用户走查，FAIL 按票重开。
