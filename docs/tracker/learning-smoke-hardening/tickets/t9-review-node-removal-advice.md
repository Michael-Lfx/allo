---
id: t9
title: "评审「移出本批」建议机械执行——同批依赖类问题摘节点不重裁"
type: task
status: open
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

t7 后 grow 剩余唯一实际死因（2026-10-20 三跑定位，干净库复现）：评审对
「节点内容依赖同批兄弟所教概念」的 reject 建议是**逐字可机械执行**的——
「把该节点移出本批，放到……已入账之后的批次」（run-1792473152116 call 3，
fresh 库实录；同款在污染库三跑中各出现一次）。这类问题走模型重裁 →
flash 重写整批 → `ng.redraft.scope` 点修越权 → 三轮耗尽。

同批依赖在**门面**已有机械剪枝先例（ng 同批依赖剪枝 + ng.title.pruned 摘
除留痕）——但那只覆盖 assumes **按名引用**兄弟 teaches 的情形；评审发现
的是**内容级**依赖（assumes 没声明），唯一正路就是摘节点。gate 摘不了
（看不见内容语义），但评审已经判定并写明了动作——代码照做即可。

**修法**（t4 同律第三刀，窄）：
- problems 处置时（t4 advice 转化之后、报告回灌之前），对 advice 含
  「移出本批」且 `problem.node` 精确匹配批内节点标题的问题：从
  `candidate.nodes` 摘除该节点，该问题从报告剔除；
- 留痕：tracing warn + 批次 note（「评审摘除：X」）——note 不在点修守卫
  比对面（t4 先例）；
- 摘空 = 空手批走既有 idle 路径（合法，不落档案）；标题不匹配或无信号词
  的问题照常打回；
- 其余问题照旧回灌重裁。

**验收**：服务层红绿样（摘除落库/无信号词照常打回两态）+ 真机两跑
grow --batches 4 EXIT=0（fresh 库口径，t7 验收顺延合并）。

## Resolution

（结案时追加）
