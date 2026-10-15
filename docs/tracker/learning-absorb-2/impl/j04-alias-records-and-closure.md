---
id: j04
title: "别名闭环全量：结构化记录 + 复用通道 + new_aliases"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# j04: 别名闭环全量

**What to build:** 概念别名从字符串升为结构化记录；铸名门获得复用通道与 new_aliases
专职入口；别名池对教练可见并带选名裁量。二轮 g3 的机制面（近邻装配 + 评审扩权 + 落池）
不变，落池数据形态按三轮 g2 升级。

**实现决议:** 二轮 g3 + 三轮 g2（修订）。参考实现：learnhub
`domain/concepts/{model,alias}.rs`、`engine/node_growth/facade.rs` 铸名门。

**Blocked by:** None（j01 的 est 改名不碰本票文件面）。

**Status:** ready-for-agent

- [ ] 迁移：`aliases_json` 元素从字符串升为 `{name, definition?, attestation?, status}`
      （存量字符串逐个回填 `{name, status:"active"}`；读写兼容旧形状一次性转换）
- [ ] **登记单点模块**（全仓唯一合法事后加名路径）：宿主存在且非废弃 / 名字全库唯一
      （**活跃占池、退役不占**）/ 幂等（已含活跃同名 = 跳过）/ **复活**（同概念退役名 =
      Reactivated 并刷新定义面）；`AliasSpec::projection` 式构造（definition = 提名定义、
      attestation = 提名 source、status = active）
- [ ] **归一化 + 近邻装配**（defnear 等价）：定义归一化（全角折叠 + 剥空白标点 + 小写）
      + 字符 trigram Jaccard + 名字近形 + 词面 top-k=3 补位（零分亦入）
- [ ] **评审门扩权**：对每条 mints 提名裁决「复用 | 新铸」——复用必须指向候选集内
      concept_id（机械核验禁编造）；**无新 LLM 调用**（撑在既有评审调用上）；评审
      fails-open 降级时退化现状（不更差）
- [ ] **复用即落池**：裁决复用 → 不铸新行，提名 canonical + 未入池别名落为该概念活跃
      别名记录；`apply_batch` 并发幂等重查路径同规则
- [ ] **new_aliases 通道**：教练批输出第三键 `{name, definition, source}`（非空）；
      归一化撞判据 = 直定宿主（不送裁）→ 近邻裁决**必须裁复用**（裁新铸 = 整批打回
      指点改走 mints）；撞本批新概念 = 直接报错；登记表空/无近邻 = 打回
- [ ] 校验条款：`ng.aliases.required` + `ng.aliases.atomic_name`（一物一名 + 批内同池
      唯一：与节点 slug、概念名、其它别名互撞即红）；`ng.refs.resolvable` teaches 域扩为
      「既有活跃别名 ∪ 批内提名」
- [ ] **名字面唯一性不放松**：撞既有活跃名仍拒铸（`ng.mints.collision`），拒铸消息补
      指引「该名字已属概念 X，直接引用即可」；apply 复用/解析查询补 `deprecated = 0`
      （二轮 g3 口径修复）
- [ ] **池可见 + 选名裁量**：教练上下文【概念登记表】块增【概念别名池】行（`正名=别名1/
      别名2`，多名概念才列、超限截断）；COACH_SYSTEM 增选名条款（选最贴合角度的已有名 /
      经提名复用引入新投影名）；别名池带定义（供 j07 的 R3 投影词表）
- [ ] 读侧：概念表/概念视图别名列带定义与出处；跨课程反查按投影名 ref 分组
- [ ] **层收缩原则**：CONTEXT.md Concept Registry 词条 + 域文档写明「登记表只存判定所需
      的定义与名字事实，教学材料归内容层」
- [ ] 验证：`cargo test -p nomifun-learning`（learnhub 测试口径移植：机械复用落池/名池
      豁免/批内改挂落池/别名池可见/归一化直定宿主/裁决新铸打回指点/幂等与复活）+
      `bun run typecheck`

**Resolution:**（收口时写）
