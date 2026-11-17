---
id: a3
title: 后台运行不再禁用折叠钮 / 不强制展开（修订 f3 Q5 子项）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

用户裁决推翻 f3 Q5 的「processing 强制展开 + 折叠钮禁用」子项：后台运行
（结构/生长/自动准备）不该剥夺用户折叠头部的控制权——「这种后台运行的
内容为什么要干扰用户进行学习」。

## 已核验事实（2026-11-17 对码）

- 现状：`headerForceExpanded = processingPrimary !== null || growing`
  （:722）→ processing 在场即强制展开（:761/:723）且两钮 disabled
  （:772/:961）。
- **强制展开的初衷（f3 Q5「徽章不被折叠吞」）已被折叠摘要条自身满足**：
  折叠条本就渲染 processingBadge（:971 兜底），展开态也有（:836-837）
  ——徽章在两态都不丢，强制展开失去存在理由。
- 放大器：b1 未修时跨课程串扰会把别课任务点亮成本课 processing，用户
  在完全无关的场景也失去折叠控制（b1 修后仍不应禁用——本课真在跑时
  用户照样可能想折叠读正文）。
- 与 a1 的关系：a1 Terms R-b 语义矩阵「processing 恒展开」一行由本票
  修订——a1 施工时按本票语义合并（processing 不再是折叠状态机的否决
  输入，只是徽章信息源）。

## Terms（方案草案，施工前过裁决）

- **R-a（主修）**：删除 `headerForceExpanded` 对展开态的强制与对钮的
  禁用——`headerExpanded` 回到 `!headerCollapsed`（a1 落地后 =
  `persisted || auto` 的结果）；两钮恒可点；processingBadge 两态照常
  渲染（现有两处渲染面零改动）。
- **R-b（状态机简化）**：`headerCollapsed` 语义只剩 persisted ∪ auto
  （a1）两层；processing 变量仍保留用于徽章/次要行渲染，不再进折叠
  判定。
- **R-c（不做）**：零节点声明窗（DeclarationWindow）分支不涉及折叠
  （zeroNodes 时结构区本就不同），不动；徽章渲染位置不动。

## Acceptance

- header-collapse 测试修订：processing 期间可折叠、折叠后徽章仍在摘要
  条、钮不被禁用（原「processing 强制展开」用例反转）。
- typecheck / i18n / theme / dead-css 绿（预期零新词条）。
- 真机目验随 a1/a2 波欠账一并走查。

## Resolution

已施工收口（2026-11-17，feat/learn1116：`c553974fe`）。

- `headerForceExpanded` 删除：`headerExpanded` 回到 `!headerCollapsed`
  （a1 落地后 = persisted ∪ auto 的结果）；两钮恒可点（disabled 干预
  删）；processingBadge 两态渲染零改动（:836 展开 / 折叠摘要条兜底本
  就在）。processingPrimary 变量保留（徽章/次要行信息源）。
- 测试反转落地：processing 期间可折叠、折叠后摘要条徽章仍在、钮不禁
  用；本地 growing 与服务端 growth_running 分用例覆盖（o6 徽章只认服
  务端真相）。
- f3 Q5「强制展开」子项正式废除留痕（本票裁决）；b1 修复后徽章只在
  本课真有任务时出现，双保险成立。
