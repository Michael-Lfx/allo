---
id: t4
title: "评审建议机械转化——advice 指向「裁为别名」的问题由代码执行"
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

评审（flash）对近义铸名的诊断准确、修法写得完整（真机实录：「方案二：按
复用裁决将该铸名裁为既有『二进制转十进制』的别名，不另铸新行」），但它
（a）把这类发现放进 problems 打回而非 reuse 裁决（提示词已引导，遵循是概
率性的），（b）即便进入重裁也常触发点修越权。每个这类问题烧一轮重裁预算。

**修法**：problems 处置前，对 `field ∈ {teaches, mints}` 且 advice 可机械
解析出「铸名 X → 宿主 Y」的问题，由代码直接执行转化——从 mints 摘除 X、
登记 X 为 Y 的活跃别名（走 j04 `register_alias_tx` 单点）、节点引用改写为
Y；该问题从报告剔除，剩余问题才回模型。解析面从窄：只认 advice 里的
「别名/复用 + 宿主名（登记表可解析）」形态，解析失败的问题照常打回。

**设计裁决点（实施前定案）**：
1. 宿主判定用登记表名解析（与 t3 的登记表全量口径一致）；
2. 机械转化不进点修守卫的 previous 基线（同 reuse 先行先例：红裁路径不改
   candidate，转化在裁决数据面随行）；
3. 转化留痕（tracing warn + 批次 note）。

**验收**：真机深批次中「近义铸名」类 problems 不再触发重裁（tracing 可
见转化记录）；服务层单测红绿样（advice 可解析/不可解析两态）。

## Resolution

（结案时追加）
