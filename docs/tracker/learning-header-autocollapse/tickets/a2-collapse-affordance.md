---
id: a2
title: 折叠钮显性化（hit area + Tooltip；右移备选）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

现有折叠/展开钮太隐蔽：展开态是标题左侧 12px 裸图标（`IconDown
text-12px !px-2px`，:766-776），折叠态是摘要条最左 12px `IconRight`
（:955-970）——用户截图红箭头标注仍难发现。自动折叠（a1）之外的兜底：
钮本身要可被发现。

## 已核验事实（2026-11-17 对码）

- 两钮均无 Tooltip、无文字标签、hit area 极小（`!px-2px`）。
- aria-label 已有（learningGraphHeaderCollapse/Expand）——无障碍面存在，
  视觉面缺失。
- 展开态标题行右端已有控件排（改名/宽度/模型选择）。

## Terms（方案草案，施工前过裁决）

- **R-a（裁决 2026-11-17：按推荐，最小组合先行）**：
  - 两钮 hit area 加大（`!px-2px` → 常规 small 钮 padding，图标 12px→14px）；
  - 各加 Tooltip（「收起头部」/「展开头部」——复用 aria-label 同源词条，
    不新增键）；
  - 折叠态摘要条的展开钮从最左移到摘要文本之后（视觉动线：先读摘要再见
    钮）或保持原位仅放大——施工按目测定。
- **R-b（备选，若 R-a 真机仍不显眼）**：展开态收起钮移到标题行右端控件排
  （与改名/宽度/模型选择同排）——远离正文起点，「收起的是上面这坨」指向
  更准；折叠态展开钮右移同理。
- **R-c 不做**：不加文字常驻标签（版面成本）；不动 a1 的自动折叠逻辑。

## Acceptance

- 渲染测试：Tooltip 存在、hit area 类断言；双 locale（不新增键则跑
  gen:i18n 确认键面不变）；theme / dead-css 绿。
- 真机目验：不开 a1 时手动折叠路径可发现（随本波真机欠账一并走查）。

## Resolution

已施工收口（2026-11-17，feat/learn1116：`4ad5c47dc`）。

- 两钮 hit area：`!px-2px` → 常规 small padding，图标 12px→14px；各加
  Tooltip（复用 aria 词条 learningGraphHeaderCollapse/Expand，零新
  键——12555 键面不变经 gen:i18n 确认）；折叠态展开钮移到摘要文本之
  后、课程名转纯文本 truncate span（R-a 首选位）。
- 测试面裁量（留痕）：bun test 宿主 Tooltip 悬停显形不可测（ESM 提升
  先于 jsdom globals 注入 → arco isServerRendering 加载期恒 true，
  Portal 恒 null）——按仓库先例改 structure test 断 Tooltip 接线 +
  渲染测试断 DOM 面（hit area / 图标尺寸 / 钮序 / 点击往返）。
- 真机可发现性欠账随波走查；不显眼再升级右移方案（票面 R-b 备选在
  案）。
