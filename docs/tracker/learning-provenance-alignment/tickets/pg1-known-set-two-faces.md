---
id: pg1
title: 出处门 Stage 1——已知集两面拆开：在册 ≠ 已知、直接出处、自教同节点、声明值
type: task
status: closed
assignee:
blocked_by: [pg0]
part_of: map.md
---

## Question

本仓 `term_provenance` 的机械核（m3，learnhub #169/ADR-0033 移植）把
「跨课程已教 ∪ 已会声明 ∪ **全登记表在册名**」压成一个
`provenance_known` 集合，于是：

- 只在登记表里、从未被任何节点教过/声明过的概念，可以合法充当
  `已教` 出处（在册 = 已教）；
- `已教` 行只核 ref 可解析，不核 `term` 与 ref 是同一概念——父概念/
  整体/描述短语可以冒充直接出处；
- `自教` 域是全批所有节点 teaches 的并集，批内兄弟可以互相供给
  （与评审自己的「批内兄弟不供给」条款相左）；
- 已会声明被并进已教账，审计面分不出「教的」与「声明的」。

隔壁 2026-11-26 出处门 Stage 1 已把同构 v1 的这四刀全部落地并给出真机
读数（learnhub #289/#290/#293，main@52e95cb）。本票把机械域对齐到本仓
自己的 CONTEXT 口径：**登记表只作身份/命名，已知集 = 已教 ∪ 已会声明**
（CONTEXT.md:96、:237；Known Names 是内容边界，不是已知集，CONTEXT.md:245）。

## 已核验事实（2026-11-26 对码）

### 名池是一个平集合

`crates/backend/nomifun-learning/src/service/learning_graph.rs:3851-3870`：

- `coverage` = 全部存活课程 teaches（`:3626-3638`）+ 已会声明概念
  （`:3649-3668`，按 `ConceptTier::Teach` 并入）；
- `registry_names` = 全库 `learning_concept_registry WHERE deprecated=0`
  的 canonical + 活跃别名（`:3672-3690`、`:5504-5518`，无课程过滤）；
- `provenance_known = coverage.keys() ∪ registry_names ∪ 本批铸名/提名`。

### 四条规则现状

| 规则 | 现状 | 证据 |
| --- | --- | --- |
| `已教` 只认 covered | **否**：名池含全登记表 | `provenance.rs:298-316` 只查 `known.contains(ref)` |
| 直接出处 term==ref/别名 | **否**：term 从不与 ref 比 | 同上；测试 `anchored_row_keeps_taught_ref_semantics`（`provenance.rs:917-935`）以 term=作用域、ref=百分数… 过门 |
| 自教同节点 | **否**：全批 teaches 并集 | `provenance.rs:190-199` + `:327-332`；提示词 `mod.rs:1055` 写「本节点」，契约行 `:1059` 写「本批某节点」——文/码已不一致 |
| 声明独立来源 | **否**：只有四值，声明并入已教 | `TERM_SOURCES`（`provenance.rs:35`）；`service/learning_graph.rs:3649-3668` |

### 会被牵连的消费面

- `derive_naming_list` / 碎片掩码用同一个 `provenance_known`
  （`service/learning_graph.rs:2697`、`:2735`）——若把「在册名」从已知集
  拿掉，必须同时保留一个**命名池**（登记表活跃名 + 本批名），否则登记表
  名字会全部变成未着陆片段（假阳性）。
- 评审判据 #2「前提足够」用的是已教账 ∪ 节点 assumes（`mod.rs:1112`），
  不是本平集合；本票不动它（隔壁同口径：Stage 1 审计面收窄不得删复用
  既有定义的前置核）。
- `rs2` 常识行机械归一用的也是这个平集合 + 全批 teaches 并集
  （`provenance.rs:250-300`）——必须同步拆，否则归一会把「在册未教」
  重新写成已教/自教，等于没修。
- 挂号表覆盖面本来就只含 mints/new_aliases 的**定义**（`mod.rs:1041`）；
  intent/scope 走碎片表。审计面收窄本仓已完成，本票不动。

## 影响面（2026-11-26 对码）

### 生产面（5 个文件 + smoke 读数）

| 文件 | 触点 | 改动性质 |
| --- | --- | --- |
| `learning_graph/provenance.rs` | `TermSource`/`TERM_SOURCES`（`:35-54`）、`validate_term_provenance` 签名（`:154`）、`rs2` 归一（`:250-300`）、`ProvenanceReadout`（`:83-98`） | 加 `Declared` 值与三面域参数；归一同步拆；读数加字段 |
| `learning_graph/mod.rs` | `GateOutcome` 字段（`:1355-1380`）、`TABLE_OUTPUT_CONTRACT`（`:1052-1062`）、`CRITERION_ONE_TABLE_CLAUSE`（`:1041`）、`:1055`/`:1059` 文/码对齐、`:1024` 四入口措辞、`review_gate()` 夹具（`:3985`） | 字段拆分 + 提示词改口径 |
| `learning_graph/growth_contract.rs` | `review_schema()` 的 `source` enum（`:635`）与三面互钉测试（`:911-920`） | 加值 + 同步断言 |
| `service/learning_graph.rs` | `gate_batch` 装配（`:3851-3886`）、`derive_naming_list`（`:2697`）、命名候选（`:2735`）、`GrowthRunStats` 填充（`:3218`）、`gate_batch` 已知面参数（`:3904-3919`） | 装配三面域；命名消费改吃 `pool` |
| `service/mod.rs` | `GrowthRunStats` 字段（`:188-194`） | 加 4 个读数 |
| `learning_smoke.rs` | 评审读数打印（`:1562-1579`） | 加 4 个读数 |
| `service/basis_tests.rs` | 读数断言（`:800-803`）、声明概念出处测试（`:746`） | 断言同步；声明测试按新值改写 |

### 符号热度（`grep` 计数，含测试）

- `TermSource` 87 处、`validate_term_provenance` 35 处、
  `ProvenanceReadout` 22 处、`provenance_known` 17 处、
  `TERM_SOURCES` 5 处——**除 `provenance.rs` 与其调用面外，非测试代码
  零外部消费者**（`TermSource::` 在 `provenance.rs` 之外零命中）。
- `provenance.rs` 自带 ~35 个测试；`known()`/`legal_rows()` 夹具
  （`:468`、`:485`）要改成三面；`anchored_row_keeps_taught_ref_semantics`
  （`:917`）、`common_sense_row_hitting_taught_pool_normalizes_to_taught`
  （`:628`）、大小写归一测试（`:716`）会随口径改写。
- `GateOutcome::default()` 在 `mod.rs` 测试里作缺省夹具（`:3985`）；
  新增字段用 `Default` 可保持既有测试构造零改。

### 不受影响

- **碎片面**：`fragment_verdict.rs` 的 `batch_taught_names` 是批内
  teaches 名集，与名池无关；本票只改它的**命名清单输入**（继续吃
  `pool`），不改裁决语义。
- **评审判据 #2**（`mod.rs:1112`）用已教账 ∪ assumes，不读名池。
- **教练面**：`taught_summary`/`roster`/`terrain` 渲染不变；`已教概念账`
  本来就把声明并在一起（用户面口径），本票只改评审挂号表的来源值。
- **DB/迁移/UI**：零改动。
- **档案兼容**：`term_provenance` 是评审回包的行内 JSON，不落库；
  旧回包只经 serde 解析（`已教` 保留 alias），无 schema 迁移。

### 风险

- **翻转面**：把「在册」从 known 拿掉会让一批原本放行的 `已教` 行
  翻红；pg0 先量，执法档按「有地板 enforce / 纯零 report-only」降级。
- **命名池误删**：若把 `pool` 也删掉，登记表名字会全变未着陆片段——
  本票明确保留 `pool` 只服务命名掩码/rs2 事实矛盾核。
- **rs2 归一漏改**：归一是第二处用平集合的地方，只改校验不改归一会把
  在册未教洗回已教/自教。


## 方案（实施相位）

### 1. 已知域拆三面（结构）

在 `GateOutcome`（`learning_graph/mod.rs:1360-1380`）里拆出：

- `covered: HashSet<String>` = 已教 ∪ 已会声明（canonical 小写）——
  `已教/已会声明` 行的出处域；
- `declared: HashSet<String>` = 本空间已会声明（canonical 小写）——
  `已会声明` 行的专有域；
- `pool: HashSet<String>` = 登记表活跃名/别名 + 本批铸名/提名——只服务
  命名掩码、rs2 事实矛盾核、复用裁决候选；**不得**再当 known 用；
- 名字解析面：`canonical_of: name → canonical`、`names_of: canonical →
  活跃名集`（从 `registry_entries` + 本批 mints/new_aliases 装配）——
  直接出处核与自教同概念判定用。

原 `provenance_known` 字段删除或改名，禁止留下一个「什么都能查」的单
集合；`derive_naming_list` 等命名消费点改吃 `pool`。

### 2. 四条机械规则

- `已教`：ref 必填，必须落在 `covered`；ref 是本批提名仍拒（保留）。
- `已会声明`：ref 必填，必须落在 `declared`；ref 是本批提名仍拒。
- 直接出处（`已教/已会声明/自教` 行）：`canonical(term) == canonical(ref)`
  ——term 等于 ref 本身或其活跃别名；父概念/整体/描述短语本阶段不作出处。
- `自教`：`allowed` = 「教该行 concept 的节点」的 teaches 交集（按概念
  身份折叠；多节点取交集；无节点教 = 空集）。ref ∈ `allowed` 才过；
  ref 指向 concept 自身仍拒（循环自证）。
- 纸面提名（`mints/new_aliases` 无任何节点 teaches）：可继续要求行，
  但其 `自教` allowed 为空——定义里的词只能走已教/声明/常识/缺失。
- `rs2` 归一改用同一三面：命中 `allowed` → 自教；命中 `declared` →
  已会声明；命中 `covered` → 已教；只命中 `pool` 且未教未声明 → 维持
  原拒/报告（不把在册洗成已知）。

### 3. 执法档（建议，按 pg0 读数可调）

- **有地板**（`covered` 非空）：四条新规则 enforce（Err → 带死因重试）。
- **纯零**（`covered` 为空，典型 = 空库第一门课首批）：四条新规则
  report-only（进 findings/readout，不挡门），沿用本仓 T1 已验证的
  「表义务」与首批纪律；避免把「首批无任何已教账」误判成批缺陷。
- 既有开关 `NOMI_LEARNING_REVIEW_PROVENANCE=off` 继续整表退出；不新增
  第二把开关。执法档进日志（enforce/report-only + 原因），进 smoke 读数。

### 4. 提示词 / schema / 测试三面同步

- `TABLE_OUTPUT_CONTRACT`（`mod.rs:1052-1062`）：
  - 来源值：`已教|已会声明|自教|常识|缺失`（新增「已会声明」；旧
    `已教` 保留为 legacy 解析值）；
  - `已教` ref 只认已教账；`已会声明` ref 只认已会声明；
  - 新增直接出处句 + 反例（父/整体/描述冒充）；
  - 自教句改为「教本概念的节点自己的 teaches；多节点取交集；批内兄弟
    不供给」，与 `:1055` 现有自查句对齐；
  - 修掉 `:1059`「本批某节点」的宽口径措辞。
- `TERM_SOURCES`（`provenance.rs:35`）+ `review_schema()` 的 enum
  （`growth_contract.rs:617-640`）+ 提示词三者互钉测试同步。
- serde：`已教` 继续可解析（archived replay/旧录制），新增
  `已会声明`（建议同时接受 `declared` 别名）；未知值仍拒。
- `ProvenanceReadout` 增读数：`registry_only_refs`、`direct_violations`、
  `sibling_self_taught`、`declared_rows`；smoke 输出行同步
  （`learning_smoke.rs:1562-1579` 附近）。

## Acceptance

- 单测：
  - 在册未教概念作 `已教` ref → 拒（有地板）/ findings（纯零）；
  - 别名解析：term 是 ref 的活跃别名 → 过；父/整体短语 → 拒；
  - 自教 ref 由兄弟节点教、非本概念节点教 → 拒；本概念节点教 → 过；
    两节点教同一概念、交集外 → 拒；
  - 已会声明概念作 `已会声明` ref → 过；未声明 → 拒；
  - `rs2` 归一：在册未教不归一；声明命中归一为「已会声明」；
  - legacy `已教` 回包可解析；`TERM_SOURCES`/schema/提示词三面一致；
  - 既有红绿样（循环自证、缺失行放行、提名缺行、常识/缺失逐字落地）
    一个不翻；`anchored_row_keeps_taught_ref_semantics` 按新规则改写
    （锚若作 ref，须在 covered/declared）。
- `cargo test -p nomifun-learning` 全绿；`cargo check -p nomifun-learning`。
- 真机/档案回放对照：`learning-smoke review-replay --replay-archive`
  对 T1 档案行前后各跑一遍；方向不得负向（放行锚仍放行、打回锚仍打回），
  新读数表进票面评论。
- 无 DB 迁移；无 UI 改动；`NOMI_LEARNING_REVIEW_PROVENANCE=off` 行为逐字节
  不变。

## Out of scope

- Stage 2 的 derivation 表 + admission 账本（`subsumed/lexicalized/plain`）
  ——pg0 读数决定是否另开票。
- Stage 3 primitive/ostensive/bootstrap——本仓无链接器（`mod.rs:686`
  「不做定向补源调度」），不搬。
- 表完备性硬门（把未落行的定义词全部挡下）：本仓碎片面缺省 report-only，
  加硬门会改变那个已裁定的默认；另票裁决。
- 已会声明的块作用域（`subsumed`）/首批参考面裁剪——pg2。
- 复用既有定义的前置核（评审判据 #2）——保持现状，不并入本表。

## Resolution（2026-11-26 施工）

**结论**：两面容差拆分落地；`covered` 规则 enforce，直接出处/自教同节点
先 report-only（pg0 实测直出 63.2%）；`stage1-enforce` 可一键全执法；
真机校准欠账。

### 落地

- `provenance.rs`：`TermSource` 增 `已会声明`（serde `declared`；旧 `已教`
  保留 `taught` alias、`常识` 保留 `plain` alias）；`TERM_SOURCES` 五值；
  新增 `ProvenanceDomain`（`covered` / `declared` / `pool` / `canonical_of`
  / `names_of`）与 `Stage1Mode`（`staged` = covered enforce、直出/同节点
  report-only；`stage1-enforce` 全 enforce；`report_all` 回放臂）。
- 机械核：`已教` ref 只认 covered（在册未教 → report-only/计数，enforce
  档拒）；`已会声明` ref 只认 declared；`已教/已会声明/自教` 行 term 须
  与 ref 同概念（别名归一）；`自教` ref 只认「教该 concept 的节点」teaches
  交集（兄弟零供给）；`batch_names` 拒绝本批铸名/别名充作出处；rs2 归一
  优先级 = 同节点自教 > 已会声明 > 已教，只在册未教不再归一。
- 装配：`GateOutcome.provenance_pool`（命名面）+ `provenance_domain`
  （已知面）；`service/learning_graph.rs::gate_batch` 从 coverage/
  declared/registry/batch 装配三面；`derive_naming_list` 继续吃 pool。
- 提示词：`TABLE_OUTPUT_CONTRACT` 五值 + 直接出处反例 + 同节点自教；
  `CRITERION_ONE_V2/TABLE_CLAUSE` 同步；schema enum 随 `TERM_SOURCES`。
- 读数：`ProvenanceReadout` / `GrowthRunStats` 增 `declared_rows` /
  `registry_only_refs` / `direct_violations` / `sibling_self_taught`；
  smoke 读数行同步。
- 测试：`provenance.rs` 36 项（含 8 项 Stage 1 新用例）；`mod.rs` 提示词/
  锚点测试更新；`growth_contract`/`basis_tests` 全绿。

### 验证

- `cargo test -p nomifun-learning --lib`：**1137 / 0**。
- `cargo check -p nomifun-app`：通过。
- `bun run typecheck` + learning UI 测试：529 / 0。
- `bun run check`：全绿。
- `NOMI_LEARNING_REVIEW_PROVENANCE=off` 行为不变；默认 = staged
  （pg0 实测直出 63.2%，covered 0.3%）。

### Code-review 修复（2026-11-27）

- 执法档可观察：`concept_review` 每次评审打一行
  `provenance stage1 mode`（stage1/covered_nonempty）；smoke 读数行加
  `stage1=`（`covered-enforce` / `report-all` / `enforce-all`）。
- `ProvenanceDomain.pool` 只含登记表活跃名+本批名（不再混入 coverage-only
  名；命名面仍用 `provenance_pool`，掩码零变化）。
- 同节点自教/循环自证按**概念身份**（canonical）判：`new_aliases` 提名名
  是既有概念别名时不再误落兄弟臂；新增回归测试。
- 收窄实现面：删掉未读的 `names_of`；`taught`/`plain` 英文 alias 删除
  （无生产者，只留 `declared` alias）；`validate_term_provenance` 与
  `render_terrain_list` 标 `#[cfg(test)]`（生产只走 domain / with_declared
  入口）。
- 回放臂限制：`replay_concept_review` 用 `GateOutcome::default()` +
  known_names，`from_known_names` 把 known 全放 covered ∪ pool，故
  `registry_only_refs` 在回放里结构上不可测；covered 规则的真机校准只能走
  live 档案/真机冒烟（已记入欠账）。

### 欠账

- **真机校准未跑**：提示词改写后的直出合规率未测；默认保持
  report-only（直出/同节点），待一次小样本
  `NOMI_LEARNING_REVIEW_PROVENANCE=stage1-enforce` 回放/冒烟确认后再翻
  缺省。`stage1-enforce` 已可用（开关即执法档）。
- 表完备性硬门未做（碎片面 report-only 默认，另票）。
