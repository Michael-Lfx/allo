---
id: z02
title: "点名四源与范围外裁决——来源标记、三值裁决、三件机械核、评审条款、拆步条款"
type: task
status: open
assignee:
blocked_by: [z01]
part_of: ../map.md
---

# z02: 点名四源扩面 + 「范围外」三件机械核

**What to build:** 机械点名清单从「只扫 mints/aliases 定义」扩到
定义∪显示名∪intent∪scope 四源，每片段携带来源标记；评审裁决词表从
两值（噪声/名词）扩三值（+范围外）；范围外裁决行过三件机械核
（按词跨源多行合法——**直接带 learnhub `20f1dcf` 真机病历修正后的最终
语义**）；评审提示词与判据①框定面同步；拆步软条款落 redraft 修订纪律。
跟随既有 `review_fragments_enabled` 开关，零新开关。

## 裁决（推荐项默认裁决——问题未获回答，票面标注可复议）

- **范围外机械核用 `20f1dcf` 最终语义，不吸 v1 初版**：合法性 =
  该词在机械清单里存在任一 intent/scope 来源的在场行（按词、跨源
  多行合法）+ 行报来源须为边界来源 + term 逐字命中对应字段原文。
  初版「行报来源与机械首行一致」单行硬比已证伪（同词双源裁决被核伪
  → 两连重试 → Degraded 保守放行整场评审被跳过）。
- **拆步条款落点 = redraft 修订纪律 + 评审拆分建议条款**（allo 无
  selfcheck phase）：learnhub 落 node-growth-selfcheck 提示词；allo 的
  拆分发生在评审打回后的重裁（`growth_body` redraft 臂），条款织入
  `:792` 修订纪律块与评审「难度诚实」判据 4 的拆分建议 advice 通道。

## 现状（2026-11-05 实码核实，基线 939443a1d）

- `fragment_verdict.rs:175-193` `derive_naming_list`：**只扫**
  `batch.mints ∪ batch.new_aliases` 的 definition × grounding_runs
  （known 掩码 + 课程豁免 `apply_ignores`），返回 `Vec<String>` 无来源；
  `:158-169` `naming_list_block` 渲染编号清单。
- `:47-66` `FragmentRuling` 两值（噪声 Noise/名词 Noun，serde 中文
  rename）；`FragmentVerdict { term, verdict, note }` 无 source 字段；
  `:93` `validate_fragments`（子串互含 `relates`、未处置分档
  report-only/enforce、名词放行=自相矛盾恒打回）；`:71` FragmentReadout
  四读数（fragments/rows/unhandled/degraded）。
- `service/learning_graph.rs:3316-3337` derive_naming_list（服务侧拼
  known 面）；`:2285-2292` naming_list 注入（`review_fragments_enabled()`
  off 臂零派生）；`:2304` fragment_readout.merge。
- 评审提示词：`mod.rs:905-917` 碎片裁决输出契约段（词表「噪声|名词」、
  行形状 `{term, verdict, note}`、缺数组/漏片段=结论无效带死因重试恰一次
  →Degraded 保守放行留痕）；`:858-862` CRITERION_ONE_V2 判据①框定
  （「节点标题与 purpose，以及随批 mints 的 canonical 与 new_aliases
  的名字及定义」——**散文面已框定标题**，机械面却只扫定义，本票顺带
  把机械面追平散文面）。
- 拆分建议通道：`mod.rs:921` 判据 4「内容过载可附拆分建议」；redraft
  修订纪律 `mod.rs:792`。
- 开关：`review_fragments_enabled()`（m9）；enforce 档
  `crate::lexicon_enforce_enabled()`。

## learnhub 参考（main@20f1dcf）

- `src-tauri/src/engine/node_growth/reviewer.rs:62-77`：
  `FRAGMENT_VERDICTS = ["噪声","名词","范围外"]`；
  `FRAGMENT_SOURCES = ["definition","node_title","intent","scope"]`；
  `FragmentSource` 枚举 + `is_boundary()`（仅 intent/scope）。
- `:706-770`（`20f1dcf` 后最终形态）`check_out_of_scope_row(fv, naming,
  batch)`：①`naming.iter().any(|f| f.source.is_boundary() &&
  relates(&fv.term, &f.term))` 否则拒（「没有 intent/scope 来源的机械
  在场行」）；②行报 `source` 缺席或非边界来源拒；③term 逐字命中
  `batch.nodes[].intent/scope`（row_source 定字段）原文。
- `:1485-1530` 病历形态测试 `out_of_scope_multi_source_term_accepts_any_
  boundary_citation`（intent「为后续方程建模备料」+ scope「不引入方程」
  同词双源，引 scope 判范围外=合法）。
- `:775-800` validate_naming 分派：范围外行→机械核；名词行→pass=false
  且 problems 指名；词表外裁决=未按词表作答视同未处置。
- validator.rs `ng.node.brief` 消息「进评审机械点名扫描」——门与评审
  的互钉措辞。
- 拆步软条款：`43056f6` node-growth-selfcheck「拆分产物不要再造新并列」
  （对照型豁免保留：判据一例外「分辨 A 与 B」类合法；「写成 X 的形式，
  并指出 Y」类先后动作串=又一处合装）；`20f1dcf` 前置病历：拆分产物
  两字段照抄原条目会让新名词零扫描进图（四源扩面的动机之一）。
- `b5d433f` GLOSSARY 词条：节点意图/教学范围（声明面、规划与写作只读
  消费、范围外点名≠引用）。

## 实施要点

### A. 片段来源标记（fragment_verdict.rs）

- [ ] 新 `FragmentSource` 枚举四值（definition/node_title/intent/scope，
  小写 serde）+ `is_boundary()`；`derive_naming_list` 返回
  `Vec<NameFragment { term: String, source: FragmentSource }>`（跨条
  去重保序：同词多源保留多行——范围外核的「跨源多行」数据基础；
  渲染可折叠同词但核验面保留全行）。
- [ ] 四源扩面：mints/aliases 定义（definition）+ 批内节点 title
  （node_title）+ 节点 intent + 节点 scope。title/intent/scope 的
  切片复用 `grounding_runs`（known 掩码 + 宁滥勿缺语义不变）；
  课程豁免 `apply_ignores` 照旧对全源生效。
- [ ] `naming_list_block` 渲染带来源标记（如 `1. 方程 [scope]`——
  格式实施时定稿，评审提示词条款同步描述该格式）；smoke 读数
  fragments 计数口径不变（按片段行数）。

### B. 三值裁决与范围外核（fragment_verdict.rs）

- [ ] `FragmentRuling` + `#[serde(rename = "范围外")] OutOfScope`；
  `FragmentVerdict` + `#[serde(default)] source: Option<FragmentSource>`
  （缺省 None 容旧行；范围外行 None = 核拒）。
- [ ] `check_out_of_scope_row(fv, naming, batch)` 三件机械核——逐条照
  learnhub `20f1dcf` 最终语义移植（见参考；relates 子串互含同款）；
  违者 = 结论不可用走评审重试（与既有死因重试链同缝）。
- [ ] `validate_fragments` 分派扩：范围外行→机械核；词表外裁决照旧
  视同未处置。FragmentReadout 口径不变。
- [ ] 单测**必含病历形态**：同词双源（intent+scope 都含「方程」）
  裁决行报 scope 判范围外 = 通过（钉死单行硬比回归）；词仅在
  title/definition 谎报 scope = 拒；term 未逐字命中 scope 原文 = 拒；
  行报非边界来源 = 拒；跨源多行引用任一边界源 = 过。

### C. 评审接线（service/learning_graph.rs + mod.rs 提示词）

- [ ] 服务侧 `derive_naming_list:3316` 返回类型随 A 改；naming 注入
  与 merge 调用点适配；`concept_review` 签名里的 naming_list 类型
  同步（`mod.rs` concept_review 函数）。
- [ ] 碎片裁决条款 `mod.rs:905-917`：词表三值 + 行形状加 source 字段
  （范围外行必填）+ 范围外裁决规则（「范围外=该词在本批某节点的
  intent/scope 声明里被明确划出边界——写『不教积分』不是使用积分；
  只出现在标题/定义里的正向用词不构成边界声明，不得判范围外」）+
  机械核描述（违者=结论无效带死因重试，与挂号表同路）。
- [ ] CRITERION_ONE_V2 `:858` 框定面补 intent/scope（「节点标题与
  purpose、以及每节点的 intent/scope 声明」）；判据①的出处核对与
  范围外的关系条款：scope 划出边界的词不算该节点的无出处缺口
  （范围外点名≠引用——防判据①与范围外裁决互相矛盾打乒乓）。
- [ ] enforce/report-only 两档行为不变（范围外核失败在 enforce 档
  合成打回、report-only 档降 findings 留痕——沿既有分档）。

### D. 拆步软条款（mod.rs 两处，零机械门）

- [ ] redraft 修订纪律 `:792` 追加：「拆分产物不要再造新并列——拆分
  产物的新显示名避免引入原条目没有的并列词（你拆的是合装，不是把
  合装换个位置重写）；确属对照/对比型连接的合法保留（『分辨 A 与 B』
  类）；两字段照各自那一半的实际内容逐半重写，禁止照抄原条目」。
- [ ] 评审判据 4（`:921`）拆分建议 advice 条款补同方向措辞
  （建议拆分时给出逐半后的 intent/scope 供 redraft 消费）。
- [ ] 单测：substring 断言两处条款在场（提示词资产断言先例）。

## Non-goals

- 不动 L0/L1（y01 one_name 三层）与结构侧点名——本票只动生长批
  评审的碎片裁决面。
- 不做 learnhub 里程碑日志（map 雾区）。
- 不新增开关；`review_fragments_enabled` off 臂行为零变（无清单则
  无范围外裁决，三值词表不出现在提示词——off 臂提示词条款裁剪照
  既有两值同款处理）。

## 验收

1. `cargo test -p nomifun-learning` 全绿，新测全过（病历形态测试
   必在场）。
2. 真机 grow 臂 EXIT=0：范围外读数行在场（fragments
   total/rows/unhandled/degraded 口径不变，degraded=false）。
3. 开关 off 臂回归：行为与基线逐字节一致（零派生）。
4. 并行约束：与 z03 无文件交集（map §文件冲突面），merger 串行并回。

## Resolution（双轴评审修复轮注记）

「词表外裁决」的 allo 形态与 learnhub `20f1dcf` 的 String 词表逐行
「视同未处置」**结构性不同**：allo 走 serde 枚举解析面拒绝（整结论
作废 → 死因重试恰一次 → Degraded），不存在「词表外词静默放行」面。
基线两值时代 allo 即解析拒（「照旧」语义），本次移植行为零变化，
**不采纳** learnhub 的 String 词表逐行「视同未处置」形态。

## Resolution（2026-11-05 关票）

- **实现提交**：`3afc01628`（单提交），经 `69c3f2d98` 并回集成分支。
- **验证**：`cargo test -p nomifun-learning` 847 全绿（新测 12：fragment_verdict 9 + mod.rs 端到端 2 + 判据①框定 1 + 拆步断言 2）；`cargo check --workspace` 净；off 臂回归（行为与基线一致）。
- **病历形态测试双层在场**：同词双源（intent+scope）裁决报 scope=过（端到端+纯函数）；词仅 title 谎报 scope=拒；term 未逐字命中=拒；行报非边界来源=拒；缺 source=拒。`20f1dcf` 最终语义（按词跨源多行合法）逐条对照移植，单行硬比未回流。
- **真机 grow 臂 EXIT=0**：`[fragments: total=127 rows=127 unhandled=0 degraded=false]`——四源点名 127 片段全处置（含 intent/scope 源），跨源语义实战零核伪零 Degraded。
- **结构注记**（双轴评审修复轮已注记于票尾）：「词表外裁决」在 allo 走 serde 枚举解析面拒绝（基线两值时代即如此），与 learnhub String 词表的逐行「视同未处置」结构性不同——行为零变化，不采纳 learnhub 形态。
- 拆步软条款与 z01 生长侧逐半句差异化落位（redraft 修订纪律 + 判据 4 拆分建议），substring 断言在场。
