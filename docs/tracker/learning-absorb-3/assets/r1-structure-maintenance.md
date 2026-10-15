# learnhub 结构维护体系深挖（#52 战役，fb72157..f776cd7，44 提交）

> 调研日期 2026-10-14；为票 [learnhub 结构维护体系深挖](../tickets/r1-structure-maintenance.md)
> 的结案资产。

## 1. 挂载管线（#95）

### 1.1 挂载事实 schema（`engine/maintenance/mounting.rs`）

```yaml
generated_at: 2026-10-13T12:00:00Z   # 本轮落盘时点（write_mounting 铸，调用方不供时钟）
structure_hash: 6f1c…                 # structure.yaml 字节哈希（FNV-1a 64，手写实现跨工具链稳定）
mounts:                               # 只记「评估过」的节点；缺席 = 未评估
  - node: 01ARZ3NDEKTSV4RRFFQ69G5FAV  # 节点 ULID
    blocks: [ownership, borrow]       # 块 slug 列表；[] = 结构外
    reason: 该节点讲所有权规则的实战运用，落在所有权与借用两块
```

机械决策：单写者 = 维护 agent（与生长写者分离）；hash 用手写 FNV-1a（`DefaultHasher`
不保证跨版本同值，持久化用它会让工具链升级平白触发整课重评）；写 = 读改写整文件原子
替换（`merge_mounts` 纯函数按图内节点序 upsert：本轮评估条目替换、未评估旧条保留、
图外孤儿清出，重跑幂等）；坏档 = Broken fail loud、文件缺席 = 空（合法）。
**挂载无判定身份红线**：不进门禁/调度/完成判定/生成判定；不铸概念；结构与概念不直连，
一切结构↔概念经节点两跳。

### 1.2 维护回合（GLOSSARY 词条 + `maintenance/mod.rs`）

> 结构与挂载的**显式增量维护**：唯一触发 = 显式动作（命令入口），整课扫描待维护面——
> **不内联生长批**（滞后无害）、不进调度、不参与任何生成判定。

两任务同骨架（RoundRetry 单源 `mod.rs:62-94`）：MAX_ATTEMPTS=3；失败整回合重试不降级
直写；**同错指纹早停**（第 N 次失败原因与 N-1 次相同 = 反馈回灌已无效，提前 Exhausted）；
回灌两种——调用失败版只带死因、违规退回版带「逐条修正 + 上一轮全文（不得原样重交）」；
预算耗尽磁盘无半成品。观测面每次调用（含失败）归档 `<obs>/llm-calls/`。

挂载回合输入七件：宪章三项 + structure_reference（`- 块名(slug)|主题一句|难度档|主干|
无前置` 参考行）+ pending_nodes（`- 显示名(slug)|所教: 名@档位,…|批次 N`）+
existing_mounts（悬挂标 `失效:slug`、空表标 `结构外`、归档标 `已归档`）+ retry_feedback。
**待挂集** = 非归档 ∧ (未评估 ∪ 有失效 slug)；stale = 全量；**待挂集空 = 零 LLM 零写
（skipped）**。输出 `{"mounts":[{node,blocks,reason}]}`（complete_json + schema 单源）。

### 1.3 st.mount.* 条款（契约 §3 ↔ validator 互钉）

| 条款 | 机械读法 |
|---|---|
| `st.mount.top.keys` | 顶层键 = {mounts} |
| `st.mount.entries.required` | node/reason 非空、blocks 字符串数组（可空） |
| `st.mount.entries.align` | 每待挂节点恰一条：无缺失、无重复、无清单外 |
| `st.mount.blocks.resolve` | slug ∈ 当前块表 slug 集逐字命中（块名/概念名不作数） |
| `st.mount.blocks.dedup` | 同条目内 slug 不重复 |
| `st.mount.no_concept_fields` | **红线扫描**：条目键 ⊆ {node, blocks, reason}——概念名/身份/teaches/assumes 禁入 |

全量报告不早停；违规报告原文注入重试装配。

### 1.4 三态失效降级

- 三态单源 `NodeMountState`：NotEvaluated / Outside（空表 = 已评估结构外，如实显影）/ 
  Mounted（逐 slug 解析）。
- **悬挂 slug 逐条降级不 Broken**（`ResolvedBlock` 三键 Option）；语义：「结构可整体退役
  是既定语义」。
- 整体待重评估：`is_stale` = 有挂载条 ∧ hash 不符 → 读侧照旧展示已挂条只加标记
  （`mounts_stale`），维护回合按新结构全量重挂；无 hash 旧档判 stale。
- 孤儿条目读侧忽略，下回合被 merge_mounts 清出。
- #96 基础件 `mount_index`（块 slug → 已挂节点清单）。

### 1.5 structure-mount 提示词核心（`prompts/maintenance/structure-mount.md`）

「挂载是**导航事实**，不是判定」；「**0 条是合法答案**……挂错比挂空更有害，拿不准宁可
少挂」；「blocks 引用结构全表 slug 逐字（不是块名，更不是概念名）」；「以节点实际讲什么
为准……结构是参考地图不是边界」；禁概念字段。temperature=0.0 + thinking=true；四条测试
矩阵（占位符互锁/金样/围栏词表/契约互钉）。

出参 `MountRoundSummary{course, evaluated_count, outside_count, mounts_total,
structure_hash, stale, skipped}`。

## 2. 结构修补（#97）

### 2.1 七型 ops（契约 §2.2 + `engine/maintenance/ops.rs`）

模型面顶层键仅 `ops`，**ops 可空 = 「评估过、本轮无修补动作」合法结论**（空数组不写盘，
防「无修补也逼出伪造 op」）：

| op | 语义 | 引用面机械自洽（引擎职责） |
|---|---|---|
| add_edge | 加边（type ∈ assumes/composes/contrasts，tier ∈ know/use/teach） | — |
| remove_edge | 三元组唯一命中即删，不命中 = 拒回合 | — |
| flip_edge | 方向纠正，端点对调，type/tier/note 保持 | — |
| add_block | 补块发**新 slug** | — |
| split_block | 拆块，旧 slug 退场 | 引用旧块的边随块退场（需要的关系由加边补回），级联注记 |
| rename_block | 只动块名（**键闭禁 slug**） | 引用该名的边端点跟改 |
| merge_blocks | 合并，旧 slug 退场 | 端点改指新块 + 同三元组去重 + 自环退场 |

**核心纪律「ops = 意图，引擎管自洽」**：`apply_ops` 纯函数（零 LLM 零 IO 零时钟）依序
应用；任一 op 无法应用 = 整体失败。块引用 = 块名逐字（编辑面看现状）；slug 只出现在
新块定义且必须新发。

### 2.2 findings 机读留痕（契约 §1.4 + `findings.rs`）

载体 = 观测面 `<obs>/findings/<课程ULID>.jsonl`（追加）；行 = `{at_unix, course, source,
node, field, issue, advice}`。**生产方 = 生长评审门 warn 通道**（数据缺陷不打回只留痕）；
**消费方 = 修补任务输入源**。坏行跳过 warn 不 Broken；留痕是线索不是第二真相。

### 2.3 st.ops.* 条款

| 条款 | 机械读法 |
|---|---|
| `st.ops.shape` | 顶层键 = {ops} 可空；逐型必填字段非空 |
| `st.ops.endpoints` | 边 ops 两端 ∈ **该 op 时刻的块表名集（应用序模拟：前序块 ops 即时进端点域）** |
| `st.ops.reason` | 每条 op 必带非空 reason |
| `st.ops.apply` | **执法在门面**：依序应用后结构全量 validator 重过（gd.* 零放宽），任一违规拒整回合 |
| `st.ops.no_slug_churn` | rename 键闭禁 slug；add/split/merge 新 slug 不得复用当前（含已退场）slug |

### 2.4 StructureRepairer 门面流程

触发门槛：findings 非空 ∪ 挂载观察非空 ⇒ 跑 LLM；皆空 = skipped。装配八件（宪章三项 +
structure_snapshot 投影 + findings 行 + mount_observations + retry_feedback）→ RoundRetry
≤3 → 全绿提取 RawOps → ops 空 = noop（LLM 已跑、不写盘）→ apply_ops → gd.* 全量重过 →
结构原子写（唯一权威）→ 从新盘面重解析 → 渲染品确定性重渲（allo no-op）→ 算 new_hash、
`mounts_stale`（**本回合不动挂载**，下回合全量重挂）→ 审计档案（快照 hash 迁移 + 逐 op
含理由 + 级联注记 + 应用后校验结论）。

挂载观察：结构外单元 + 空块（0 节点）+ 超载块（≥5 节点挂载）；「仅观察，不构成编辑义务」。

**级联口径（ADR-0017）**：修补先落 → 挂载本回合不动 → hash 变 = 既有挂载待重评估 →
下回合全量重挂。**不建逐 op 跟迁规则**——「挂载无判定身份，误编只伤导航，失效降级本即
既定语义」。

structure-repair 提示词：「结构编辑是低风险维护」；「每个操作一条依据：从留痕或观察出发，
输入里没有依据的编辑不做」；「不需要任何编辑时输出空数组」；地图自身下限（块数 40–100、
一物一名、slug 唯一）；「编辑会被全量复核……宁可少改、改准」。

出参 `StructureRepairSummary{old_hash, new_hash, applied[], cascade[], mounts_stale, noop, skipped}`。

## 3. 概念视图与三视图跳转（#94/#96）

- `ConceptView{id, name(展示名=首别名), definition, def_src, deprecated, superseded_by,
  aliases: Vec<{name, definition, def_src, attestation}>, related: Vec<RelatedRefGroup>}`——
  跨课程反查扫全空间挂接行，**按投影名 ref 分组**（缺失或 = 展示名归正名组，组名 = 展示
  名）；组序正名组首、组内课程新→旧；零新存储纯读。
- 节点结构位置段（三态 + 失效条 + 待重评估横条）、区块已挂节点清单 + 计数、五图已挂数
  徽标（emerald 单源配色，`mounted_count<=0` 不落图元；徽标 = 读数指针不是行为入口）。
- 架构依赖边：`export` 只读消费 `mounting::MountIndex`（单向零写，导出永不落第二真相）。

## 4. concepts 域重构

### 4.1 层字段收缩（契约 decomposition §4.1 v2.9 / node-growth v1.11）

`ConceptRecord { id(ULID), definition(唯一主判据), def_src, deprecated, superseded_by,
aliases }`——**去 operation/example/pitfall**：「教学材料归内容层（内容管线产物），登记表
只存判定所需的定义与名字事实」。六件套血统归概念条目（v2.8）。旧档三键读取忽略、重写
脱落。

### 4.2 别名携带定义与来源

```rust
pub struct AliasRecord {
    pub name: String, pub lang: Lang, pub kind: AliasKind,  // canonical/translation/abbreviation/former/colloquial
    pub definition: Option<String>,      // 该名字的意义（投影角度定义；缺席 = 与概念定义同义）
    pub def_src: Option<DefSource>,      // ai/human
    pub attestation: Option<String>,     // 名字出处
    pub display_priority: u32,           // 投影通道默认 5
    pub status: AliasStatus,             // active/retired
}
```

语义：别名 = **概念的投影命名面**（一次学习只证明概念某角度某深度的投影）；装配显示
`正名=别名1/别名2`，模型负选名之责；挂接 ref 记录实际所用名，读侧据此分辨哪些投影有
学习证据。

### 4.3 批内 new_aliases（给 LLM 的别名教学通道）

输出第三键 `new_aliases: [{name, definition, source}]`（`ng.aliases.required`）——「别名
不是新概念：宿主由铸名门按定义判定」。铸名门处理：
1. `definition` 归一化（全角折叠 + 剥空白标点 + 小写）撞既有判据 = **直定宿主**（不送裁）；
2. 否则进与 new_concepts 同一条近邻裁决（归一化精确/近形/trigram ≥0.4 软提示 + 词面
   top-k=3，恒送裁），**必须裁为复用**——裁「新铸」= 整批打回并指点改走 new_concepts；
   撞本批新概念 = 直接报错；登记表空/无近邻 = 打回；
3. 放行经**登记单点** `alias::register_alias`（全仓唯一合法事后加名路径）：宿主存在且非
   废弃、名字全库唯一（**活跃占池/退役不占池**）、幂等（AlreadyPresent 跳过）、**复活**
   （同概念退役名 Reactivated 并刷新定义面）；`AliasSpec::projection` 构造函数统一默认
   （kind=colloquial、priority=5、def_src=ai、attestation=提名 source）；
4. 校验：`ng.aliases.atomic_name`（一物一名 + 批内同池唯一：与节点 slug、概念名、其它
   别名互撞即红）；`ng.refs.resolvable` teaches 域扩为「既有活跃别名 ∪ 批内提名」。

### 4.4 display 指针废除

展示名恒 = 别名集首条（铸造序正名在前）；「不为不存在的用例保留第二展示名真相源」。

## 5. 契约 v2.7 / v2.8 / v2.10 与 ADR-0017

- **v2.7 结构转活文档**：结构落盘从「生成时点快照（显影剂）」修订为「可增量维护的活文档」
  ——维护 agent 可增量编辑（validator 全量门 + 观测审计 + Git 出处）；可整体退役/重画
  （人工低频批处理、双裁决留痕）；不建受理门/覆盖层机器。
- **v2.8 术语纠偏**：块 = 主题单元（KSB 原子判据与六件套血统归概念条目）；收敛率/
  铺设利用率/「转正」退役。
- **v2.10 门位口径**：从零声明课程，门划在**学习者真实起步地板**；门到目标之间的一切
  （含预备层）都是课程内内容、进结构（开局贴地、**可学可挂**），地形收缩为地板以下与
  自检参考；写作面指导不加机械门。实证：数学课反例（门划初中、结构纯高中 → 地面不教
  不挂 = 错）vs 十批正例（78 块含地面段、开局贴地）。**门位口径是挂载的前提**。
- **ADR-0017 七裁决**：①节点为三段 join 枢纽；②挂载独立存储单写者；③结构↔概念不直连；
  ④结构转活文档；⑤触发 = 显式维护回合；⑥结构编辑自动落盘 + 全量审计；⑦修补 v1 = 边
  ops + 块 ops（原裁块级后置，因级联已由 hash 兜住并入 v1）。否决：挂载入节点侧字段
  （双写者）、受理门机器、生长批内联挂载、块-概念直连网。

## 6. smoke-mount / smoke-repair 判据

- smoke-mount：①落位 + hash 锚定 + 条数一致；②结构外计数照印（合法）；③**重跑幂等**
  （第二次必须 skipped，否则「增量扫描失效」EXIT=4）。
- smoke-repair：①回合成功（skip/noop 照印）；②**盘面复验**（磁盘重读独立跑全量 validator）；
  ③渲染一致（渲染品 = 重算逐字节相等）；④**级联标记**（结构已改时挂载必须判 stale）。

## 7. #89–#93 摘要

- **#89 错题归因**（ADR-0018）：归因 = 账本派生投影（零新状态）；图只出候选、证据定
  culprit（只有①级探测题失败才移因到前置；拒绝罚分沿依赖传播、不上 BKT）；投影级归因
  （作答 → (course,node) → teaches × ref）；分级升级 + 延迟分型（保持/流利/链断）。
- **#90 Quiz 会话**（ADR-0019）：触发 = 距上次 quiz 的 node-completed 数 ≥ K（起步 6，
  复习与 quiz 自身账不推进触发）；测量非门禁；弱项优先全序采样，逐节点 1 旧题 + 1 变式
  （变式追加入库），≤15 题。
- **#91/#92 复习形态与阶梯 + 护栏**（ADR-0020/0021）：drill/reference（开卷）/relearn 三
  形态，**开卷不重复结算**；阶梯只在链断启用（R1 = 同题跨日连败 ≥3 天；R2 = 投影窗口
  正确率 <1/2）；阶梯顶 = 铸新触发留档。护栏（零 LLM）：债务刹车（债 > 40 题卡停教新）、
  空窗回归（≥3 天空窗 → 先一次 quiz、清债 ≥70% 才恢复）。
- **#93 掌握度消费面**：只读切片（证据按投影分组）；④级自报与①级机器证据并列披露不
  合并；**消费面白名单 = 只为人看/解释**；禁用面三条：不做门禁、不做抽样权重、不与 XP
  通兑。
