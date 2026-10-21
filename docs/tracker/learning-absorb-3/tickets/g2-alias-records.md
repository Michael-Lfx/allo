---
id: g2
title: "概念别名结构化升级（修订二轮 g3）+ new_aliases 通道"
type: grilling
status: closed
assignee: zhangyj
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

## Resolution

> **裁决方式注记（2026-10-14）**：连续第三轮盘问未获回答（用户以「推进」节奏驱动），按
> 四项推荐项默认裁决——皆有 learnhub 真机实证背书且为可逆文本/结构决策；票面标注可复议。

**速览**：四项全采推荐——别名轻量结构化四字段、new_aliases 通道采纳、层收缩原则入域
文档、展示名不改。

1. **别名记录轻量结构化**（修订二轮 g3 的落池形态）：`aliases_json` 元素从字符串升为
   `{name, definition?, attestation?, status}`——learnhub 六字段中 lang/kind/
   display_priority 在 allo 无消费方，不进（留演进空间）。语义沿用 learnhub：
   `definition` = 该名字的意义（投影角度定义；**缺席 = 与概念定义同义**）；`attestation`
   = 名字出处（提名的 source）；`status` = active/retired（**退役不占名池**——与二轮
   g3 deprecated 口径衔接）。落池动作写入的记录 = `{name, definition: 提名定义,
   attestation: 提名 source, status: active}`（learnhub `AliasSpec::projection` 同构）。
   **迁移**：存量字符串逐个转 `{name, status: "active"}`（071+ 段位）。
2. **new_aliases 通道采纳**：教练批输出加第三键 `new_aliases: [{name, definition,
   source}]`（三字段非空）——给既有概念提名新名的专职入口。铸名门处理链照 learnhub：
   ①提名 definition 归一化（全角折叠 + 剥空白标点 + 小写）撞既有判据 = **直定宿主**
   （不送裁决）；②否则进二轮 g3 的近邻裁决，但**必须裁为复用**——裁「新铸」= 整批打回
   并指点「这不是别名，改走 new_concepts」；撞本批新概念 = 直接报错；登记表空/无近邻 =
   打回；③放行经**登记单点**（全仓唯一合法事后加名路径）落表，四保证照抄：宿主存在且
   非废弃、名字全库唯一（活跃占池/退役不占）、幂等（已含活跃同名 = 跳过）、**复活**
   （同概念退役名 = Reactivated 并按本次提名刷新定义面）。校验条款：
   `ng.aliases.required` + `ng.aliases.atomic_name`（一物一名 + 批内同池唯一——与节点
   slug、概念名、其它别名互撞即红）；`ng.refs.resolvable` 的 teaches 域扩为「既有活跃
   别名 ∪ 批内提名（mints ∪ new_aliases）」——同批提的别名立即可被引用并按 ref 落位
   （二轮 g2 的引用名开讲数据源）。
3. **层收缩原则入域文档**：allo 登记表本来只有 definition（无六件套字段），天然合规——
   把「登记表只存判定所需的定义与名字事实，教学材料归内容层」写进 CONTEXT.md
   Concept Registry 词条与学习域文档。
4. **展示名不改**：allo canonical 优先 ≡ learnhub 首别名铸造序，语义等价。

**派生影响**（供合并切票）：迁移（aliases_json 元素形状升级 + 存量回填）；铸名门/评审门
扩展（new_aliases 键 + ng.aliases.* 条款 + 裁决面挂接）；登记单点模块（register_alias
等价物，唯一合法事后加名路径）；教练上下文别名池行升级（`正名=别名1/别名2` → 可带
投影意义，供二轮 g2 的 R3 投影词表）；概念表/概念视图读侧（别名带定义与出处列）；与
二轮 g3/t1 同批实施。

