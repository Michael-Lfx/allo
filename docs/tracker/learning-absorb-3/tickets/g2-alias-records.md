---
id: g2
title: "概念别名结构化升级（修订二轮 g3）+ new_aliases 通道"
type: grilling
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

二轮 g3 已裁「别名闭环全式：近邻装配 + 评审扩权 + **字符串落池**」未实施。learnhub #52
的 concepts 域重构把别名升级为**结构化记录**（`{name, lang, kind, definition, def_src,
attestation, display_priority, status}`——别名 = 概念的投影命名面，携带该名字的意义与
出处），并加了**批内 new_aliases 通道**（教练可给既有概念提名新名：归一化直定宿主 →
近邻裁决必须裁复用 → 登记单点：活跃占池/退役不占/幂等/**复活**）+ display 指针废除
（展示名 = 首别名）+ **层字段收缩**（去 operation/example/pitfall——教学材料归内容层，
登记表只存判定所需的定义与名字事实）。

需要裁决：

1. **别名记录结构化**：二轮 g3 的落池从 `Vec<String>` 升级为结构化记录（新迁移或重建
   `learning_concept_registry.aliases_json` 的元素形状）？learnhub 的六字段里 allo 真正
   有消费方的：definition（该名字的意义——R3 投影词表/概念视图需要）、status（退役不占池
   ——g3 已裁 deprecated 口径）、attestation（名字出处——审计面）；lang/kind/
   display_priority 在 allo 无消费方（可存缺省或裁掉）。**推荐**：采纳 {name, definition?,
   attestation?, status} 四字段轻量版（kind/lang/priority 不进，留演进空间）。
2. **new_aliases 通道**：教练批输出加第三键（给既有概念提名新名）——与二轮 g3 的「评审
   扩权裁决复用」是同一裁决面的两个入口（mints 提名复用 vs 专职别名提名）。**推荐采纳**
   （learnhub 的「必须裁复用否则整批打回指点改走 new_concepts」纪律 + 登记单点四保证
   直接照抄；二轮 g3 的近邻装配由两者共用）。
3. **层字段收缩**：allo 的登记表 definition 字段已有；无 operation/example/pitfall
   （本来就没引入）——此条 allo 天然合规，只需把「登记表只存判定所需」写进域文档。
4. **展示名 = 首别名**：allo 现展示名 = canonical；learnhub 改为首别名（正名铸造序在首
   等价于 canonical 优先）——语义等价，无需改。

约束：本票是**对二轮 g3 决议的修订**（升级而非推翻——近邻装配 + 评审扩权 + 落池三个
机制面全部保留，落池的**数据形态**升级）；登记单点的四保证（宿主非废弃/名字全库唯一/
幂等/复活）是新增机械细节，照抄 learnhub。
