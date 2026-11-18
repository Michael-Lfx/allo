---
id: l1
title: 匹配题行布局——label 列基准宽修正 + select 宽度约束
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

匹配题左列被压成 ~80px 窄柱（长短语竖排三四行），下拉框占满其余全部
宽度。排版怎么修？

## 已核验事实（2026-11-18 对码）

- 根因：MatchingInput 行 `flex items-center gap-8px` 里 label
  `<span className='min-w-80px flex-1'>`（flex:1 1 **0**，基准 0）+
  Arco Select 默认 `width:100%`（basis≈整行）——收缩按 shrink×basis 分
  摊，label 塌缩到 min-width、Select 吃满。
- ActivityInput 课时练习/复习卡片共用，两处同坏。
- 题面长短语是常态（如「执行「新创建一个对象」后得到的对象」）。

## Terms（施工定案）

- **R-a（主修）**：label 列给真实基准宽——`flex-[1_1_40%]`（40% 基准，
  可收缩可增长，长短语在 ~40% 宽度换行而非 80px 竖排）；Select
  `flex-[1_1_60%] min-w-140px`（占余下 60%，仍保留 min-w 防极窄）。
  行保持 `items-center`；换行多的 label 用 `items-start`? —— 按「行内
  居中 vs 顶对齐」目测定，默认 `items-center` 不动（单行 label 居中更
  自然，多行 label 视觉可接受）。
- **R-b（一致性留观，不在本票）**：matching 左列/候选项为裸文本，其它
  题型选项走 InlineMarkdown（:157/:176）——若后续要对齐另开小票，本票
  只动布局不动渲染管线。
- **R-c 不做**：不换组件（Select 保持）；不改判分数据面（label/select
  value 仍是原始串）。

## Acceptance

- 渲染测试：长短语 label 行——label 元素宽度显著大于 min-width（断言
  basis 类生效 / 或 getBoundingClientRect > 80px）；select 不过度占满
  （有界）；两处消费面（课时 + 复习卡）回归绿。
- typecheck / theme / dead-css 绿。

## Resolution

已施工收口（2026-11-18，feat/learn1116）：MatchingInput 行布局 label
`min-w-80px flex-[1_1_40%]`（40% 真实基准，长短语在 40% 宽度换行而非
80px 竖排）+ Select `min-w-140px flex-[1_1_60%]`（余下 60% 有界）；
注释留痕根因（flex-1 基准 0 被 Arco Select width:100% 的 basis 吸收全
部收缩量）。渲染测试 +1（长短语断言 flex-[1_1_40%]/flex-[1_1_60%]/
min-w-140px），6 pass；typecheck / theme / dead-css 绿。课时练习 +
复习卡两消费面同组件同修（R-c 判分数据面零改动；R-b InlineMarkdown 一
致性留观不在本票）。真机复验：匹配题左列正常换行、select 宽度合理。
