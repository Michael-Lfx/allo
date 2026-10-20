---
id: t9
title: "评审「移出本批」建议机械执行——同批依赖类问题摘节点不重裁"
type: task
status: closed
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

**速览**：全项落地（commit `b7c4ff7f2`），learning 344 全绿，**t7 遗留的
「连续两跑」验收在本票落地后达成**。

- **执行面**：problems 处置序（t4 advice 转化 → 撞池合入之后）——advice
  含「移出本批」且 `problem.node` 精确匹配批内节点标题 → 摘除该节点、剔
  除该问题；留痕 = tracing warn + 批次 note「评审摘除：X（移出本批）」。
  摘空走既有空手批 idle 路；标题不匹配或无信号词照常打回（红绿样 ×2：
  摘除落库 + 不匹配早停）。摘除后的 candidate 成为点修守卫的新基线
  （previous）——后续重裁只对照摘后批面。
- **真机时间线**（干净库，flash）：t9 前两跑 round 3 死于本类
  （run-1792473152116 等）；t9 后 f3 跑中 **removed=1 实战救场**（该轮
  免折返直接落库），最终 **f3（19 节点）+ f4（16 节点，round 4 含一次
  守卫原地重试）连续两跑 grow --batches 4 EXIT=0**——smoke-hardening 波
  的 grow 收官验收达成。
- **九档矩阵**：structure/grow/loop/repair/quiz(双环境)/content/xp/json
  全部有本波或历史 EXIT=0 回执——loop 档 2026-10-20 补跑（--rounds 2 + 凭据库）EXIT=0（结构 + 2 生长批全绿）。
- **残余（记录不扩票）**：语义类打回中「挂名缺失/概念内容错位」两形态
  （需模型改写内容，非逐字可执行）仍走重裁，flash 点修纪律失守时烧预
  算——发生率已从「每跑必死」降到概率性；候选后手（产品波）：把守卫
  失败的整批降级为「摘除全部被点名节点」而非重裁。
