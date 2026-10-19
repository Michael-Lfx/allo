---
id: t6
title: "结构 stage 标注一致性——入门块不得前置依赖进阶块"
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

2026-10-19 生长节点专家审计发现结构产物的 stage 标注 2–3 处自相矛盾：
`位权概念`（进阶）是 `位权序列`/`二的幂`（入门）的 assumes 前置——入门块
依赖进阶块，与「入门档从门口贴地起排」的门位口径冲突。实际生长把位权展开
提前到首批教掉了，**生长补偿了结构的错**，但补偿依赖模型发挥，不是契约。

**裁决点**：
1. **提示词条款**（goal-decomposition.md 结构规范补一条）：「stage 与
   assumes 同向——assumes 的 target 块档位不得高于 source 块」+ validator
   加一条 warn 级 `gd.stages.ordering`（warn 不 red：档位错位是质量问题不
   是结构非法）；金样重生成。
2. **或接受现状**：生长实际补偿、无用户可见伤害——在 t2 丰富度对账里记一
   笔即可。

倾向 1（一条 warn 条款成本极低，且给评审/修补回合留痕），但 warn 阈值与
「thin_spine 已有先例（红条带豁免键）」的形态一致性需实施时核对。

## Resolution

（结案时追加）
