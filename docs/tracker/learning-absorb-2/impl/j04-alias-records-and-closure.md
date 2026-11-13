---
id: j04
title: "别名闭环全量：结构化记录 + 复用通道 + new_aliases"
type: task
status: closed
assignee: zhangyj
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

**Resolution:**

**速览**：全项落地（feat/learn0928，本票单提交）。learning 271 全绿（+7）、db 全套绿
（含迁移测试 2）、typecheck 60 = 存量基线、check:i18n 过、workspace check 干净。

- **迁移 077**（`077_concept_alias_records.sql`）：①`aliases_json` 存量字符串逐个回填
  `{name, status:"active"}`（json_group_array + json_each，测试钉住回填与 json_extract
  活跃名查询路径）；②`learning_lesson_concepts.ref_name` 可空列（节点引用时的投影名，
  老行为 NULL 读侧回退正名）。读侧双读：`alias_records::parse_alias_records` 兼容旧
  字符串形状（坏元素跳过不致命），写侧恒为新形状。
- **三个新模块**（learning_graph/）：
  - `alias_records.rs`——`AliasRecord {name, definition?, attestation?, status}`（learnhub
    六字段的四字段轻量版，lang/kind/priority 留演进）+ **登记单点**
    `register_alias_tx`（全仓唯一合法事后加名路径；四保证：宿主存在且非废弃 / 名字全库
    唯一活跃占池退役不占 / 幂等已含活跃同名跳过 / 复活 Reactivated 刷新定义面——各有
    测试）+ `AliasSpec::projection` 构造式（definition=提名定义、attestation=提名出处、
    status=active）。
  - `defnear.rs`——归一化（全角折叠 + 剥空白标点 + 小写）+ 字符 trigram Jaccard（阈值
    0.55）+ 名字近形（互包含、最短 2 字）+ 词面二元组 top-3 补位（**零分亦入**）+
    `definition_exact_host` 直定宿主 + `assemble_neighbors` 候选装配（去重保序）。
- **契约扩展**（growth_contract）：顶层键白名单加 `new_aliases`；`NEW_ALIAS_KEYS`
  三字段全必填 → `ng.aliases.required`；批内共享名池（节点标题 + 铸名 + 提名）互撞 →
  `ng.aliases.atomic_name`（一物一名）；schema 同步 + 互钉测试。`ng.refs.resolvable`
  的可解析域扩为「既有活跃别名 ∪ 批内提名」。
- **门面（gate_batch）**：`registry_name_set` 升级为 `registry_entries`（概念 + 定义 +
  活跃别名，deprecated=0 单源）+ 名字宿主映射（**撞名消息带宿主指引**：「该名字已属
  概念 X，直接引用即可」）；每条提名（mints canonical + new_aliases）装配
  `NominationPlan { direct_host, candidates }`——定义归一化逐字相同 = 直定宿主（绕过
  评审）；new_aliases 无近邻 = 门级打回（`ng.aliases.no_neighbor`）。validate_batch
  签名扩 name_owner。
- **评审扩权**（无新 LLM 调用，撑在既有评审调用上）：CONCEPT_REVIEW_SYSTEM 增
  【复用裁决】节（裁复用必须照抄候选集 concept_id、编造即违约；new_alias 必须裁
  复用）；评审 user 消息注入【提名候选集】渲染（含直定宿主行）；输出契约加
  `reuse: [{name, concept_id}]`（pass/reject 均可携带，`ng.review.reuse` 形状条款）；
  `concept_review` 返回 `ReviewOutcome { problems, reuse }`——fails-open 降级 = 两者
  皆空（退化现状：reuse 丢失 = 该提名无裁决）。
- **growth_body 裁决面**：机械核验每条 reuse 裁决（name ∈ 本批提名 ∧ concept_id ∈
  候选集，**编造/越集条目静默丢弃**）；new_alias 无有效复用裁决且无直定宿主 → 整批
  打回（`ng.aliases.not_reuse` 固定文案指点「改走 mints」）——同指纹早停适用（测试：
  2 轮 4 调用即停，每轮耗起草 + 评审 2 调用）。通过后 `rulings`（提名名 → 宿主）
  传入 apply_batch。
- **apply_batch**：rulings 先行（引用解析按宿主走）+ **复用即落池**（mints 被裁复用
  = 不铸新行，canonical + 未入池别名经登记单点落为活跃别名记录）+ new_aliases 落池
  （definition=提名定义、attestation=提名 source）+ **deprecated=0 两处同口径修复**
  （二轮 g3 的原 bug：铸名复用重查与引用解析查询补 `deprecated = 0`——测试
  `retired_names_do_not_block_minting` 钉住：退役行不阻塞新铸、引用落活跃行）+
  新概念行的别名写为新形状记录 + `ref_name` 写入（节点引用时用的名字）。
- **池可见 + 选名裁量**：教练上下文登记表切片增【概念别名池】行
  （`正名=别名（投影定义）/…`，多名概念才列、超长截断）；COACH_SYSTEM 三条——
  选名裁量（别名池里选最贴合角度的名字）、new_aliases 通道用法（新角度值得命名就
  提名，裁决复用后保留为别名；不要为同一角度造第二个名字）、mints vs new_aliases
  分界（登记表没有的走 mints）；输出形状示例加第三键。
- **读侧**：`GraphConceptRowView.aliases` 升为 `GraphConceptAliasView {name, definition,
  attestation}`（活跃别名带定义与出处）；`GraphConceptRefView.ref_name`（投影名，老
  行 NULL）；前端概念表别名渲染带定义、引用行显示「以「X」引用」（i18n 双语新键
  learningGraphRefAs）；types.ts 同步。
- **层收缩原则**：CONTEXT.md Concept Registry 词条改写（别名 = 投影命名面 + 层收缩：
  登记表只存判定所需的定义与名字事实，教学材料归内容层）+ learning.zh.md 概念注册
  段落补别名闭环全貌。
- **实施偏差与边界记录**：①批内名池把「节点 slug」本地化为「节点标题」（allo 节点
  无 slug 列，标题即身份；精确相等比对，子串不算撞）；②「跨课程反查按投影名 ref
  分组」以 ref_name 落库 + 概念表引用行展示落位（完整的引用名开讲投影随 j07，本票
  只铺存储与最小展示）；③评审 reuse 裁决核验失败取「静默丢弃该条目」而非整批降级
  ——编造 concept_id 是评审违约，保守处理不让坏裁决驱动落池；④deny-side 的
  `ng.aliases.no_neighbor` 用词面补位保底后仅在登记表**真空**时触发（词面 top-3
  零分亦入的推论）。
- **测试**（learnhub 口径移植，+13）：parse 双读/坏档、register_alias 四保证、defnear
  四件（归一化/直定/近邻与词面/近形判据）、复用落池（不铸新行 + 别名带定义落池 +
  ref_name 落位）、直定宿主（评审零裁决放行 + 定义出处落池）、裁决新铸打回指点
  （早停 4 调用）、别名池可见（教练装配含池行与投影定义）、退役名不占池（新铸落
  活跃行）、ng.aliases.required/atomic_name、迁移回填。
- **验证**：learning 271 全绿、db 全套绿（含 077 迁移测试 2）、`cargo check
  --workspace` 干净、`bun run typecheck` 60 = 存量基线、`check:i18n` 过（11392 键）。
