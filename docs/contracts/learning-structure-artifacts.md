---
contract: learning-structure-artifacts
version: "1.2"
date: 2026-10-15
status: 定稿（wayfinder 票 g1/g2/g4 裁决落地；ADR-0010 与 CONTEXT.md Structure 词条为最高约束）。v1.1：契约文本随批（j01）——门位口径 v2.10 采纳（§3 + 两份结构提示词）；术语纠偏 v2.8 确认归档；R9 叙述声部与跨课预告废除属内容面条款，随 j07 落内容契约位（§8 注记）。v1.2（j09）：新增 §10 结构修补 st.ops.* 五条款 + 挂载 §9（j08）——结构转活文档完整契约（ADR-0012）
sources: [docs/tracker/learning-absorb/map.md, docs/tracker/learning-absorb-2/map.md, docs/tracker/learning-absorb-3/map.md, docs/adr/0010-endpoint-anchors-and-compass-retirement.md, learnhub decomposition-artifacts v2.10]
---

# 学习结构产物契约 v1.0

> 输入学习目标（+ 可选程度/基础声明）→ 一次生成两份产物：①**知识结构**
> （structure，spines/blocks/edges 快照）②**前置地形**（terrain，门口 G 判据）。
> 本契约约束两份产物的字段、权威面、质量门槛与存储；结构生成管线按本契约实现，
> `gd.*` 条款编号是**校验器报错、整批重试反馈、提示词自查**三方的公共语言。
> validator 与 JSON Schema 的单源实现住 `nomifun-learning` 的
> `learning_graph::structure_contract`（schema↔validator 由互钉测试锁定）。

## 0 最高约束

- **课程边界铁律（最高优先，用户 2026-10-12 强调）**：知识结构**仅限课程内部
  生效、跨课程隔离**；概念登记表**跨课程**；两层**永不直接关联**——块与边不出
  课程边界、不做跨课程复用或合并；投影不 join 登记表、不做块→概念任何解析或
  名字匹配、可视化不互渲染；结构对生长**不供内容、不做判定、不进任何门**。
  **豁免注记（2026-10-13 评审裁定）**：快照内的程度/基础声明回执标量
  （`proficiency_statement` / `learner_basis_assumed`）属用户输入面的记录，
  可被课时内容生成等管线读取以保持 grounding，不属于结构语义面——不构成
  「结构供内容」违例。
- **存储面只存**：`learning_courses.structure_json` 单字段 JSON 快照（§5）。
  **永不存储**：概念间关系、树、矩阵、分层、路线、scenarios——一切视图
  （图/矩阵/路线）运行时重算、永不落盘第二份（ADR-0002 铁律）。
- **结构源单源**：快照里的 structure 产物是知识结构的唯一权威；读侧投影与
  之不一致时以快照为准，重算即得。不产人读文档（learnhub 的 knowledge-system
  是其单机文件形态伴生物，g2 裁决：概览语义全部落在读侧投影）。
- **顺序权威唯一**：教练裁量 + 已教概念账；产物中一切层号/分层/路线不是排期，
  覆盖读数永不作判定（ADR-0010）。
- **整批重试带反馈**：任一产物违反契约，两份**一起**重新生成，且重试装配必须
  注入上一次的**逐条款违规报告**（`format_violations` 全文形态）——无反馈的
  盲目重试零收敛（learnhub #53 实证：三次重试 33 项同型违规）。

## 1 载体与形态

- 模型面：LLM 经 json 接缝（`LearningCompleter::complete_json_with_thinking`，
  response_format=json_object）产出 JSON。**provider 不支持 json_object 即建课
  失败并明示原因（fail loud，不降级）**——合法 JSON 由解码器保证，结构合规由
  本地 schema 单源 + 语义 validator 执法。
- `gd.parse` 因此是读侧条款：validator 在 JSON 值面执法（查对象形状），不查
  JSON 语法；KaTeX 长文规则不适用本契约（正文分节生成走 content-write，不在此）。
- 落盘：两份产物连同元数据装进快照信封（§5），单列存储、原子覆盖（重跑生成 =
  换参考地图不换课；节点/概念网与结构无连接，覆盖无副作用）。

## 2 产物一：知识结构（structure）

### 2.1 顶层键

白名单 14 键；`gd.keys` 管白名单与结构五件（domain/spines/blocks/gaps/quality
必填），八个标量键的缺席与为空由 `gd.frontmatter` 报（learnhub 两类条款分工）：

```json
{
  "goal": "<目标原句回执，不得改写>",
  "goal_type": "capability | coverage",
  "proficiency_statement": "<程度声明>",
  "domain_type": "理论 | 工程 | 历史 | 技能",
  "form_reference": "<结构模板出处>",
  "source_tier": "名录 | 章节",
  "generated_by": "<模型标识>",
  "disclaimer": "<一句诚实边界：出处档位与已知局限>",

  "domain": {
    "core_definition": "<核心定义一句>",
    "includes": ["<包含>", "..."],
    "excludes": ["<不包含>", "..."],
    "prerequisite_doors": ["G1", "G2"],
    "audience": "<受众>",
    "scenarios": ["<典型场景>", "..."],
    "granularity": "<粒度规划一句>"
  },
  "spines": [
    { "name": "<主干名>", "definition": "<一句定义>", "key_points": ["<要点>"],
      "instance": "<可验证实例>", "thin_spine": false, "reason": "" }
  ],
  "blocks": [
    { "name": "<块名>", "slug": "<英文标识>", "spine": "<主干名>",
      "stage": "入门 | 进阶 | 高级", "note": "<一句话主题描述>" }
  ],
  "edges": [
    { "source": "<块名>", "target": "<块名>", "type": "assumes | composes | contrasts",
      "tier": "know | use | teach", "note": "<一句说明>" }
  ],
  "gaps":  [ { "item": "<此处未覆盖>", "direction": "<补全方向>" } ],
  "quality": [ { "check": "<自检项>", "status": "met | unmet", "reason": "" } ]
}
```

**块 = 原子教学主题单元**（能独立解释一个现象或完成一个操作）。字段只有身份与
结构位（五键纪律）——定义/操作/实例/易错/source 等判据形字段一律不进块；块不是
概念，`note` 明令非判据、永不进登记表。**一物一名**：名字禁并列连词与并列分隔符
（官方术语自身含并列形的以「」包裹豁免；「和」在词首/词尾位是名词不是连词，词中
位才计并列——learnhub #53 留痕的机械读法放宽）。薄干声明随主干给
（`thin_spine: true` + 非空 `reason` 豁免每干配额）。

**边方向语义**（learnhub #42 定案）：`assumes` 的**源 = 需求方、目标 = 前置**，
一切依赖视图方向反转展示（assumes = 门）；`composes` 不钉方向、仅参考组装展示；
`contrasts` 无方向、同屏对照防混。`weight` 不进源。

**领域适配四型**：`domain_type` 由模型自判（理论/工程/历史/技能），决定维度与
关系偏好；型分支指引住提示词资产，不进本契约。

### 2.2 硬条款（validator 逐条执法，违者整批打回）

| 条款 | 机械读法 |
|---|---|
| gd.parse | 产物必须是单个 JSON 对象（读侧条款：语法合法性由 json 模式前置保证） |
| gd.keys | 顶层键 ⊆ 白名单 14 键；结构五件（domain/spines/blocks/gaps/quality）全在 |
| gd.frontmatter | 八个标量键全非空；goal_type ∈ {capability, coverage}；domain_type ∈ 四型词表；source_tier ∈ {名录, 章节} |
| gd.domain.required | domain.core_definition/audience/granularity 非空；includes/excludes ≥3；scenarios ∈ 2..=3 |
| gd.domain.doors | prerequisite_doors ≥1 条、逐条形态 `G<数字>`（对账解析见 §4） |
| gd.spines.count | spines 条数 ∈ 6..=12 |
| gd.spines.required | 每条 name/definition/instance 非空、key_points ≥1；键 ⊆ {name, definition, key_points, instance, thin_spine, reason}；name 在产物内唯一 |
| gd.blocks.count | 条数 < 40 红、> 100 红；80 < n ≤ 100 记「越档」**警告**（不拒收） |
| gd.blocks.fields | 块条目键 ⊆ {name, slug, spine, stage, note}（判据形字段 = 红）；stage ∈ {入门, 进阶, 高级}；note 非空 |
| gd.blocks.one_name | 块名不含并列标记（与/和/及/、/,/，/斜杠），「…」段内豁免、词边「和」豁免 |
| gd.blocks.slug | slug 匹配 `^[a-z0-9-]+$`；name 唯一、slug 唯一 |
| gd.blocks.spine_ref | 每块 spine ∈ spines 名集 |
| gd.blocks.quota | 每主干块数 ≥5；不足者该主干须 `thin_spine: true` + 非空 reason，否则红 |
| gd.edges.endpoints | 边两端 ∈ 块表名集（解析域就是块表，悬挂 = 红） |
| gd.edges.type | type ∈ {assumes, composes, contrasts} |
| gd.edges.tier | tier ∈ {know, use, teach}（注意与概念网 tier 的 know/apply/teach 是两套词） |
| gd.edges.dup | 同 (source, target, type) 三元组唯一 |
| gd.gaps | ≥3 条且 item/direction 均非空 |
| gd.quality | ≥5 条；status ∈ {met, unmet}；unmet 必带非空 reason |
| gd.no_schedule | 全文无排期语词（机械代理词表：排期/日程/时间表/进度/计划/小时/每天/每周/每月/个月/半天/整周；裸「天/周/月」误报高，不扫）。**quality 清单与 disclaimer 豁免**（元声明面——自检理由合法地谈论红线本身） |

警告级（不拒收、随报告透出）：`gd.blocks.count` 越档（80 < n ≤ 100）。

## 3 产物二：前置地形（terrain）

零基础衔接的独立产物：门口 G1..Gn 判据，是 structure `prerequisite_doors` 的
**唯一合法引用源**，也是教练首批的入口层重点参照（其后软参照，g3）。allo 的
地形是 learnhub 同名产物的**减配版**——消费面只有门口（TerrainViewDto 门口
列表 + 教练上下文门口块），learnhub 的地形分层/DAG/词汇表节在 allo 无消费方，
不进契约。

```json
{
  "goal": "<目标回执>",
  "learner_basis_assumed": "<基础声明回执（缺省输入时回执纯零基础假设）>",
  "generated_by": "<模型标识>",
  "disclaimer": "<一句诚实边界>",
  "doors": [
    { "id": "G1", "criterion": "<可验证判据>", "kind": "hard | self_check" }
  ]
}
```

| 条款 | 机械读法 |
|---|---|
| gd.terrain.keys | 顶层键 ⊆ {goal, learner_basis_assumed, generated_by, disclaimer, doors} |
| gd.terrain.frontmatter | 四个标量键全非空 |
| gd.terrain.doors | doors ≥1；键 ⊆ {id, criterion, kind}；id 形态 `G<数字>` 且唯一；criterion 非空且含能力动词（能/可——可验证判据的机械代理）；kind ∈ {hard, self_check}（hard = 无豁免硬前置，self_check = 可自检项） |

地形与结构同为**参考面**产物：不参与判定、不与概念关联。**门位口径（v2.10）**：门画在
学习者的**真实起步地板**上——地板 = 基线学习者今天已具备、能直接踩着开始的那条线；
「教材第一章」「正式入门」都不等于地板。门口以下是假设已会的前置（地板以下），不属于
本课程教学单元；**地板以上的一切内容全进知识结构**（开局贴地、可学可挂）——结构入门档
从门口贴地起排，门口与入门档之间不留无主地带。地形由此收缩为**地板以下与临门自检**
两类判据（`hard` / `self_check`），由生长循环自己教出来的部分照旧归生长。

## 4 门口对账

structure 的 `prerequisite_doors` 逐个编号必须能在 terrain 的 doors 定义里解析，
悬挂 = 红（违规代码仍记 `gd.domain.doors`，消息携带已知门口清单）。对账失败 =
两份产物一起重生成（生成侧机械对账，堵引用悬空——g2 裁决）。

## 5 存储与快照信封

- 落点：`learning_courses.structure_json`（TEXT NULL，CHECK 只收 JSON 对象；
  迁移 071 引入）。**NULL = 结构未生成**（词汇表 Missing：合法空状态，读侧
  200 恒答，不走错误路径；生成中状态由 WS structure_* 事件表达）。
- 信封（快照 = 一个 JSON 对象，两产物 + 元数据，单 UPDATE 原子写入）：

```json
{
  "version": 1,
  "generated_at": 1697060000000,
  "structure": { "...§2 产物..." : "" },
  "terrain": { "...§3 产物..." : "" }
}
```

- 重跑覆盖：重生成 = 原子换快照，课程行、节点、概念网不受影响。
- **投影永不落盘**：图/矩阵/路线/门口列表一律运行时从快照重算（纯函数、零
  LLM、零时钟），不建关系表存块间边（ADR-0002）。
- 快照是**生成时点显影剂**：可整体退役（置 NULL），退役不损登记表与账本。

## 6 生成轮次（g2 裁决）

① structure ② terrain 串行双调用（均走 json 接缝、thinking ON）；任一产物
违反契约或门口对账失败，**两份一起**重生成、逐条款违规报告注入重试装配，
**≤3 轮**；全部通过才落库（快照原子更新）。耗尽则**结构生成失败明示**
（WS `structure_failed` + 前端可重试的重新生成按钮），**课程保留、快照保持
NULL、无半成品落列**（建课接口不等待生成，失败不回收课程——可重试重跑）。

## 7 与 learnhub 的差异清单（全部有裁决依据，不得"补齐"回旧形态）

| learnhub v2.4 | allo v1.0 | 依据 |
|---|---|---|
| structure.yaml 文件 + knowledge-system.md 渲染品 | structure_json 单列快照 + 读侧投影 | g2 #4/#5：SQLite 单字段快照；不产人读文档 |
| pt.* 地形全节（分层/DAG/差距/词汇表/门语义） | gd.terrain.* 只保门口 | 消费面只有门口（t1 TerrainViewDto、g3 教练块） |
| 普查 / 交付账 / selectable / NoFrontier / 铸名门 | 不存在，也不得重新引入 | 铁律：learnhub 已用真机代价否决（map Notes） |
| 落盘 YAML（wire-only 转换） | JSON 直存快照列 | 单字段快照语义，无第二真相形态 |
| gd.edges.audit 参考质量告警 | 未引入（v1 观测面后置） | 契约 = validator 1:1，不纸上设条款 |

## 8 契约文本随批注记（v1.1，j01）

learnhub 窗口内四项纯文本契约演进的落点对账（三轮 t4）：

| 项 | learnhub 出处 | 落点与状态 |
|---|---|---|
| 门位口径 v2.10 | decomposition-artifacts v2.10 | **本批采纳**：§3 语义段 + `prompts/structure/goal-decomposition.md` / `prerequisite-terrain.md`（金样随批重生成） |
| 术语纠偏 v2.8 | v2.8「块 = 主题单元」 | **确认归档**：本文档 §2.1 与两份提示词的块定义已是「原子教学主题单元」口径；全仓 grep 无「转正/收敛率」残留（零命中） |
| R9 叙述声部 | contract v2.6 | **已落地（j07，allo 适配反转）**：learnhub 原文要求正文自称「本课/这一课」（其节点 = 独立课程）；allo 课时是课程内学习材料，**反转禁用**自称「本课/这一课/本课时」，开发面结构词（本节将/下一节将/接下来我们将/如前所述）与跨课预告照禁——落两条管线写作提示词（`NARRATIVE_VOICE_BLOCK` + agent 系统提示词同款）与评审判据观察条（`narrative_self_reference` / `narrative_structure_words`，非阻断）——不进本结构契约 |
| 跨课预告废除 | contract v2.6 | **已落地（j07）**：节大纲小结收本课以内、「接下来/下一课」预告废除——落 `section_outline_system` 规划条款与 content 写作面（R9 声部块内） |

est 命名统一（二轮 g1 第 6 项）同批落地：`ng.nodes.minutes` → `ng.nodes.est`、
`ProposedNode.est`、DTO/前端镜像键 `est`；**DB 列 `learning_lessons.estimated_minutes`
保留旧名**（SQLite 无列注释机制，语义注记住本条与本批 DTO 文档注释：列语义 =
est，1 est 分钟 = 1 分钟专注学习）。

## 9 挂载事实层（st.mount.*，j08 三轮 g1「结构转活文档·上」）

挂载是**导航事实**：帮助学习者知道「节点在参考地图的哪个主题块里」。它无判定身份
——不进调度、不进门禁、不承载概念事实（teaches/assumes/tier 只住节点挂接表）。
挂载与修补（j09，st.ops.*）合起来才是「结构从快照变活文档」的最小闭环。

### 9.1 载体与回合形态

- 挂载表一课一行（`learning_structure_mounts`：`course_id UNIQUE, mounts_json,
  structure_hash, updated_at`），与 `structure_json` 单字段快照先例同构；回合内单行
  原子覆盖（merge upsert：本轮替换、未评估保留、孤儿清出），无中间态。
- **structure_hash = 手写 FNV-1a 64**（勿用语言默认 hash——跨版本稳定）对
  `structure_json` 列原样字节；读侧 hash 失配 = `mounts_stale`（结构变了挂载即显影）。
- **触发 = 显式命令**（`POST /courses/{id}/graph/structure/mount`）；单课串行、可取消
  （j03 注册表，kind = `structure_mount`）。
- **待挂集** = 现存节点 ∧（未评估 ∪ 失效 slug）；hash 失配 = stale = 全量。
  **待挂集空 = skipped：零 LLM 零写**。失败整回合重试不降级（RoundRetry ≤3，同指纹
  早停——j03 骨架复用），违规报告原文注入重试装配。
- 两个防幻觉/防浪费语义照抄 learnhub：**0 条挂载是合法答案**（全在结构外也合法）、
  **待挂集空 = skipped**。

### 9.2 产物形态与六条款（validator ↔ 本章互钉）

产物 = 单个 JSON 对象：`{ "mounts": [ { "node", "block", "reason" } ], "note"? }`。
`node` = 节点标题（逐字照抄待挂集）；`block` = 块 slug（逐字照抄结构参考行）或
`null`（结构外——已评估、声明不属于任何块）；`reason` = 一句话（主题同一性判据）。

| 条款 | 内容 |
|---|---|
| `st.mount.top.keys` | 顶层键只允许 `mounts`、`note`；`mounts` 必为数组。 |
| `st.mount.entries.required` | 条目键只允许 `node`、`block`、`reason`；node 非空、reason 非空。 |
| `st.mount.entries.align` | `node` 必须逐字命中待挂集——只评估待挂集节点。 |
| `st.mount.blocks.resolve` | `block` 非空时必须逐字解析到当前结构 `blocks[].slug`。 |
| `st.mount.blocks.dedup` | 一个节点只出现一条条目（一个节点只挂一块，或声明一次结构外）。 |
| `st.mount.no_concept_fields` | 条目不得携带概念字段（teaches/assumes/tier/concepts）——挂载无判定身份。 |

### 9.3 读侧三态投影（单源）

- **Mounted**（挂入可解析块）/ **Outside**（已评估、声明结构外）/
  **NotEvaluated**（未评估；含悬挂 slug 条目的逐条降级——不使整体 Broken）。
- `mount_index`：块 slug → 已挂节点清单（只含可解析块）。
- `mounts_stale`：结构 hash 失配标记（读侧显影，不自动重评）。
- **挂载观察派生**（修补输入源之一，纯读函数不打点）：结构外单元 + 空块 +
  超载块（≥5 节点）。观察不构成编辑义务。
- 挂载行随课程删除（Cascade，纯导航派生面可重建）；findings 留痕表
  （`learning_structure_findings`，j08 建表 / j09 接生产与消费）随课 KeepHistory。

## 10 结构修补（st.ops.*，j09 三轮 g1「结构转活文档·下」）

修补 = 结构的低风险维护编辑：模型只表达意图（七型 ops），纯函数引擎管自洽；应用后
gd.* 全量重过零放宽。三重护栏 = 全量 validator 门 + 观测审计（修补审计表）+ 出处
（逐 op 含理由），不建受理门机器。**ops 空 = 「评估过、本轮无修补动作」的合法结论**
（LLM 已跑、不写盘——防凑动作）。

### 10.1 七型 ops 与级联（引擎职责）

块引用 = 块名逐字（编辑面看现状）；slug 只出现在新块定义且必须新发。

| op | 语义 | 引用面机械自洽（引擎代管，级联注记记数） |
|---|---|---|
| add_edge | 加边（type ∈ assumes/composes/contrasts，tier ∈ know/use/teach） | 同三元组已存在 = 拒 |
| remove_edge | 三元组唯一命中即删 | 不命中或多命中 = 拒回合 |
| flip_edge | 方向纠正：端点对调，type/tier/note 保持 | — |
| add_block | 补块发新 slug | — |
| split_block | 拆块，旧块退场 | 随块边一并退场（需要的关系由加边补回） |
| rename_block | 只动块名（键闭禁 slug，slug 终身不变） | 引用旧名的边端点跟改 |
| merge_blocks | 合并，源块退场 | 端点改指新块 + 同三元组去重 + 自环退场 |

### 10.2 五条款

| 条款 | 机械读法 |
|---|---|
| `st.ops.shape` | 顶层键 = {ops} 可空；逐型必填字段非空；type/tier 词表。 |
| `st.ops.endpoints` | 边 ops 两端 ∈ 该 op 时刻的块名集（应用序模拟：前序块 ops 即时进端点域；rename 的 to 是本 op 引入的新名，不算端点引用）。 |
| `st.ops.reason` | 每条 op 必带非空 reason——从留痕或观察出发，输入里没有依据的编辑不做。 |
| `st.ops.apply` | 执法在门面：依序应用后结构全量 validator 重过（gd.* 零放宽），任一违规拒整回合。 |
| `st.ops.no_slug_churn` | rename 键闭禁 slug；add/split/merge 新 slug 不得复用当前（含已退场）slug。 |

### 10.3 修补回合与级联口径

- 触发门槛 = findings 非空 ∪ 挂载观察非空；**皆空 = skipped（零 LLM 零写）**。
- 装配（宪章三项 + 结构快照投影 + findings 行 + mount_observations + retry_feedback）
  → RoundRetry ≤3（同指纹早停）→ ops 空 = noop → apply → gd.* 全量重过 → 结构快照
  原子写 → 修补审计行落 `learning_structure_repairs`（old_hash/new_hash/逐 op 含理由
  与级联注记）。
- **级联口径（ADR-0017）**：修补先落 → 挂载本回合不动 → hash 变 = 既有挂载待重评估
  （mounts_stale）→ 下回合全量重挂。**不建逐 op 跟迁**——挂载无判定身份，误编只伤
  导航，失效降级本即既定语义。
