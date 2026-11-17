---
id: b2
title: 徽章动作名与按钮对齐（生长中→补充节点中）
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

同一动作两面三个名字：徽章「生长中…」（learningGraphGrowing）、按钮
「补充节点（就绪 x/y）」（learningGraphGrow）、领域文档叫「生长批次」。
用户指出「知识结构生长」与「补充节点」不是一回事，提示应精准。

## 已核验事实（2026-11-17 对码）

- 语义分工（领域事实）：**结构生成** = 首次建结构（gd.* 契约，徽章走
  structureBadge 族「结构生成中 · 第 x/y 次」）；**补充节点（生长批次）**
  = 教练补批加节点（KIND_GROWTH，按钮 learningGraphGrow）；**自动准备**
  = 派发窗口读数（learningGraphAutoPreparing「自动准备中…」，语义另词，
  不与本票混）。
- 错位点：growth 徽章用「生长中…」（zh:458 / en "Growing…"），与按钮
  「补充节点」不同名——用户无法把「生长中」对准到「补充节点正在跑」。
- 消费点：徽章 :714 + 次要行 :690（learningGraphGrowing 两处）；
  en 同键 "Growing…"。
- b1 修复后徽章只在本课真有 KIND_GROWTH 时出现——术语修正随之才有
  意义（串扰期间无论叫什么都误导）。

## Terms（方案草案，施工前过裁决）

- **R-a（裁决 2026-11-17：按推荐，键名随更）**：growth 徽章值改与按钮动
  作名对齐——zh「补充节点中…」/ en "Adding nodes…"；**键名随值更名**
  `learningGraphGrowing` → `learningGraphAddingNodes`（grilling 定案；
  `bun run gen:i18n` 更新键面；两消费点 :690/:714 同步）。两 locale 对称
  （红线 2）。
- **R-b（词汇落册）**：GLOSSARY 增补两词条精确定义——「结构生成」（首次
  建 gd.* 结构）与「补充节点」（生长批次加节点）；「生长」从用户面文案
  退役（领域文档可留）。
- **R-c（不做）**：自动准备词不动（不同动作）；结构生成徽章族不动
  （已是精准名）；按钮文案不动（learningGraphGrow 本尊即正确名）。
- **R-d（依赖）**：呈现正确性依赖 b1（串扰期间任何名字都错）——施工
  顺序 b1 先或同批。

## Acceptance

- 渲染测试更新（新键名 + 新值双 locale 断言）；`gen:i18n` + `check:i18n`
  绿；typecheck / theme / dead-css 绿。
- GLOSSARY 两词条落册；grep 全仓确认「生长中…」用户面零残留（领域文档
  不限）。

## Resolution

（未结案）
