---
id: t4
title: "评审建议机械转化——advice 指向「裁为别名」的问题由代码执行"
type: task
status: closed
assignee: zhangyj
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

**速览**：全项落地（commit `2d8859436`），并按六次真机实录补了一处票面
外的同旨扩展（铸名撞池机械复用）。

- **advice 机械转化**：新模块 `learning_graph/advice_alias.rs`（纯函数 +
  6 单测）。窄解析面：field ∈ {teaches, mints} + 「复用/别名」信号词 +
  恰一个本批提名名（「」/『』引号段，按去重后不同名计——issue/advice 常
  各自重复点名同一对象）+ 恰一个宿主（登记表可唯一解析的引号名，或文本
  内登记表在册的 concept_id 字面量 8-4-4-4-12）。解析成功 → 合成 reuse
  裁决进 j04 落池通道，问题从报告剔除；失败照常打回。评审显式裁决优先于
  advice 补位。
- **撞池机械复用（票面外扩展，run-1792402566347 实录）**：mint canonical
  撞登记表现有名时，`gate_batch` 直接吸收对应 `ng.mints.collision` 红条并
  合入 auto-ruling（名字唯一池下复用是唯一不重命名的合法出路，模型只会
  重提同名烧光预算）。窄条件：未入池别名不撞**别的**概念（否则落池
  Conflict，照常红条）。auto-ruling 覆盖评审/advice 裁决（撞池名落别的宿
  主必然 Conflict）。
- **三条设计裁决**全按票面：宿主判定 = 登记表名解析（与 t3 口径一致）；
  转化不进点修守卫基线（不改写 candidate）；留痕 = tracing warn + 批次
  note（「评审建议机械转化 X→复用 Y」「撞池机械复用 X」——note 不在守卫
  比对面）。顺带把「裁决已消化问题」的掩码扩到『』引号风格（真机混用）。
- **单测**：服务层红绿 ×3——advice 可解析（reject 单轮即落库、不铸新行、
  别名带定义、note 留痕）；advice 不可解析（宿主不在登记表）照常打回同指
  纹早停；撞池 canonical 机械复用（红条吸收、引用落宿主、note 留痕）。
  全量 `cargo test -p nomifun-learning` 全绿。
- **真机**：结案波 grow 真机回执见 t3 票面增补（round 2-4 的近义铸名/
  撞名重裁循环正是本票靶点）。
