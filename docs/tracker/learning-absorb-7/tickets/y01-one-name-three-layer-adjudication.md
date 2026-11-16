---
id: y01
title: "one_name 三层分工——L0 硬门 / L1 点名提示 / L2 批量中立裁决"
type: task
status: closed
assignee:
blocked_by: [y02]
part_of: ../map.md
---

# y01: one_name 三层分工

**What to build:** 把结构块名的并列形态检查从「七字符一刀切硬门」拆成三层：
L0 纯标点（`、`/`，`）仍是硬门；L1 连词/分隔符（`与/和/及/,//`）只点名提示、
不单独阻断；L2 仅在 L1 是唯一阻断时触发一次**批量中立裁决**，逐名字裁定
「是捆扎还是术语本字」。裁决器不可用/输出不合契约 → 降级为提示、不阻断。
**先修结构侧真实误杀，再接生长侧提示面。**

## 现状（2026-11-04 实码核实）

- `learning_graph/structure_contract.rs:265` `compound_name_mark`：`与/和/及/、/,/，`
  七字符一律返回，`和` 仅在词首/词尾豁免；`:538` 直接产出红条
  `gd.blocks.one_name`。
- 提示词 `prompts/structure/goal-decomposition.md:32` 明确承诺
  「术语本字含并列字的照官方名整体放行（如「求和」「和差」「和角积」）」，
  并给出交卷自检句——但机械门只豁免词边 `和`，`求和公式`（`和` 在词中位）
  仍被判红。这是**提示词与硬门互相矛盾**的真实误杀，一旦触发就烧整批 3 轮。
- 修补路径 `service/learning_graph.rs:935-950` 对修补后结构做全量复核，同样
  硬拒；生长侧 `learning_graph/growth_contract.rs:350` 只有批内撞名
  （`ng.aliases.atomic_name`），**没有任何名字形态检查**；`learning_graph/mod.rs:485`
  COACH_SYSTEM 只有「一物一义」。
- 现有 LLM 裁决先例：声明面 `basis_declaration.rs:89-96`（逐名输出 + 机械核
  concept_id）、碎片裁决 `fragment_verdict.rs`（批量 + 覆盖校验 + report-only
  降级）——均无 memo、无「唯一阻断才付费」成本杠杆。

## learnhub 参考（main@22d016d）

- `src-tauri/src/engine/name_adjudication/mod.rs:67-298`：L0 `、`/`，` 硬门；
  L1 `与/和/及/,//` 只记 `NameSuspect`、不阻断；L2 仅「有 suspect 且
  violations 为空」时付费；输入只给名字+被点字符（不带位置/产物面/机械门
  结论）；输出 `{"verdicts":[{name,bundle,reason}]}`；机械校验空表/未提交名/
  重复/空理由整次不可用；运行内 memo；裁决器失败 → `Degraded` 降提示。
- 调用时机：`decomposition/facade.rs:244-258`（其他门之后）、
  `node_growth/facade.rs:901-903`（铸名门+语义评审之后）。
- 资产：`prompts/name-adjudication/name-bundle.md` + golden + 回放两例
  （`sum-formula-single-term` 放行 / `pair-bundle` 维持）。
- 注意：其契约文档仍有「`和` 仅词中位」旧句与代码矛盾，**不要照抄**；
  allo 直接取消词边豁免（L1 不再阻断，词边 `和` 交给 L2 裁）。

## 实施要点

### A. L0/L1 拆分（结构侧）

- [ ] `structure_contract.rs`：
  - `hard_name_mark(name)`：只扫 `、`/`，`（`「」` 段内豁免）→ 红条
    `gd.blocks.one_name`（保留现有出路文案）。
  - `soft_name_marks(name)`：扫 `与/和/及/,//`（`「」` 段内豁免；**取消
    词首/词尾 `和` 豁免**）→ 新 `gd.blocks.name_hint`（`warning = true`）。
  - 新增 `pub struct NameSuspect { name, mark, location }` 与
    `pub fn collect_name_suspects(structure: &Value) -> Vec<NameSuspect>`；
    `validate_structure` 只产出 L0 红条 + L1 warn（warn 不进 `format_violations`
    的生成反馈，现有调用方只取 `!warning`）。
- [ ] 修补路径：L1 warn 不阻断（与 learnhub maintenance 面一致），补一行
  `tracing::info!(name_hints = n, ...)`；L0 仍硬拒。
- [ ] 更新既有测试：`gd_blocks_one_name_reports_all_offenders...` 改为
  L0 报全；`...exempts_quoted_terms_and_word_edge_he` 改为「`求和公式` 不再
  红、以 hint 在场」；新增 L0 `、` 红条与 `「」` 豁免。

### B. L2 批量中立裁决（结构生成）

- [ ] 新模块 `learning_graph/name_adjudication.rs`：
  - `NameAdjudicator`（持 completer/model_override 与运行内 memo
    `HashMap<String, bool>`），`adjudicate(&mut self, suspects) -> Adjudication`。
  - 输入渲染只含「名字 + 被点字符」，不告知位置、产物面与机械门结论；
    prompt 判据「拿不准时倾向单一主题术语」。
  - 输出契约 `{"verdicts":[{name,bundle,reason}]}`；机械校验：每个提交名恰
    一条、无未知名、无重复、理由非空；任一不满足 → `Degraded`。
  - 失败（传输/解析/形状）→ `Degraded`，`tracing::warn!` 留痕；不得把裁决器
    错误写进生成重试反馈。
- [ ] prompt 资产 `prompts/structure/name-bundle.md` + `golden/name-bundle.md`；
  `structure_prompts.rs` 增 `NAME_BUNDLE`（temperature 0.0、无 thinking、
  `max_tokens` 小档如 2048、`FENCE_DEV_FACE`）；`REGISTRY` 与金样测试的
  硬编码四资产列表同步扩到五。
- [ ] `structure_gen.rs`：
  - `generate_structure_artifacts` 创建一个 `NameAdjudicator`，跨 attempt 复用
    （memo 生效）；`attempt_once` 增参数接收 `&mut`。
  - 位置：所有红条为空之后、`Ok(StructureArtifacts)` 之前。`suspects` 非空
    才调用；`bundle=true` → 追加红条 `gd.blocks.one_name`（带裁决理由）并
    按现有机制重试；`false` → 放行；`Degraded` → 放行 + hint 日志。
  - 同指纹早停/重试反馈只认红条；L1 hint 与裁决 reason 不单独占轮。
- [ ] 观测：生成日志带 `name_suspects={n}`、`name_hints={n}`、
  `adjudication=ok|degraded`；不新增 WS phase。

### C. 生长侧（v1 提示面）

- [ ] `growth_contract.rs` / `learning_graph/mod.rs` `validate_batch`：扫描
  节点标题、`mints.canonical/aliases`、`new_aliases.name` 的 L1 形态，作为
  新增 warn 通道（建议码 `ng.mints.name_hint`，`GateError` 现有 warn 面）
  返回；**不阻断、不触发重裁**。
- [ ] hint 文案给「拆成多条提名 / 改用无并列规范名」出路；不把 hint 拼进
  `feedback`。v1 **不接** growth L2 裁决；触发再立票（真机出现捆绑铸名且
  hint 无改善）。
- [ ] 若 `npm` 侧 smoke 有 growth OK 读数行，追加 `name_hints={n}`；没有
  则只留 tracing。

### D. 契约与文档

- [ ] `docs/contracts/learning-structure-artifacts.md` v1.8→v1.9：补
  「名字形态三层」表（L0 红 / L1 提示 / L2 裁决）+ 与 learnhub 的差异
  （growth v1 仅提示、取消词边豁免）；同步 §7 差异清单。
- [ ] `docs/tracker/learning-quality-fixes/blueprints/2026-10-30-f2-structure-prompt-polish.md`
  的「打地鼠」留档不改；在本票 Resolution 记「f2 的提示词自检句保留，机械门
  误杀由本票收口」。

## 验收

- 单测（Scripted completer）：
  - `求和公式` → L0 无红、L1 有 hint、L2 裁决 `bundle=false` → 生成通过。
  - `百分数与利率的对应` → L2 `bundle=true` → 红条 + 重试反馈含理由。
  - `「广义表与树」的辨析` → `「」` 豁免，无 hint。
  - `、`/`，` → L0 红，不进 L2（不付费）。
  - L2 输出缺名/多名/重复/空理由/传输失败 → `Degraded`，生成通过且有 hint。
  - memo：同名跨 attempt 只调用一次裁决。
  - 修补路径：L1 只 hint 不拒；L0 仍拒。
  - 生长：捆绑铸名只出 `ng.mints.name_hint`，批次照常落库。
- `cargo test -p nomifun-learning` 全绿；金样重生成幂等；契约文档同步。
- 真机（串行）：structure 档含「求和公式」正例通过、`双进程与窗口` 类
  反例仍被 L2 维持；smoke `EXIT=0`；growth 档一次捆绑名 hint 可见。

## 非目标

- 生长侧 L2 付费裁决（v1 只提示；真机证据再立票）。
- 维护/修补路径接 L2（learnhub 自身也只放行留痕）。
- learnhub 的 replay catalog/compare 夹具体系（allo 用 Scripted completer
  + 真机 smoke）。
- 把裁决输入扩到定义/上下文（保持中立，避免锚定）。

## 风险

- L2 是额外 LLM 调用：靠「唯一阻断才触发」+ memo 控成本；若真机每轮都
  触发且裁决不稳，先收窄到结构侧、growth 保持 hint。
- L1 升 warn 后，若有其他消费方把 warnings 当阻断（当前只 info 日志/
  smoke 读数），需逐个核对；`format_violations` 只格式化传入切片。
- 契约 v1.9 与金样五资产列表是强耦合点，改 `REGISTRY` 必须同步金样测试。

## Resolution（2026-11-04）

**实现点**（分支 `feat/absorb7-y01`，基于 9092c7d9a + 合并集成尖端）：

- `learning_graph/structure_contract.rs`：`HARD_NAME_MARKS`（`、`/`，`）与
  `SUSPECT_NAME_MARKS`（`与/和/及/,//`）分表；`hard_name_mark` /
  `soft_name_marks`（「」段内豁免；**取消词边 `和` 豁免**）；新增
  `pub struct NameSuspect{name,mark,location}` 与 `pub fn
  collect_name_suspects`（L0 不进清单、同名去重保首条）。`validate_structure`
  只对 L0 出红条 `gd.blocks.one_name`，L1 出 `gd.blocks.name_hint` warn。
- 新模块 `learning_graph/name_adjudication.rs`：`NameAdjudicator` 持
  completer/model_override + 运行内 `memo: HashMap<String, NameVerdict>`；
  `adjudicate(&mut self, suspects)` 批量调用（输入只渲染「名字 + 被点字符」，
  不带位置/产物面/机械门结论）；输出契约
  `{"verdicts":[{name,bundle,reason}]}`，机械校验「每个提交名恰一条 / 无未知
  名 / 无重复 / 理由非空」，任一不满足 → `Degraded`；传输/解析失败同样
  `Degraded` + `tracing::warn!`（错误不进重试反馈）。
- 提示词资产：`prompts/structure/name-bundle.md` +
  `prompts/structure/golden/name-bundle.md`；`structure_prompts::NAME_BUNDLE`
  （temperature 0.0、无 thinking、`FENCE_DEV_FACE`）；`REGISTRY` 与金样测试
  列表扩到五资产；`generation::NAME_ADJUDICATION_MAX_TOKENS = 2048`。
- `learning_graph/structure_gen.rs`：`generate_structure_artifacts` 创建一个
  `NameAdjudicator` 跨 attempt 复用（memo 生效），`attempt_once` 接收
  `&mut`；位置在「全部红条为空」之后、`Ok(StructureArtifacts)` 之前——仅
  `collect_name_suspects` 非空才调用。`bundle=true` → 追加红条
  `gd.blocks.one_name`（带裁决理由）走既有重试/同指纹早停；`false` → 放行；
  `Degraded` → 放行 + warn。日志带 `name_suspects={n}`、`name_hints={n}`、
  `adjudication=ok|degraded`，无新 WS phase。
- 修补路径 `service/learning_graph.rs`：复核后 L1 只 `tracing::info!
  (name_hints = n)`，现有 `!warning` 过滤保证 L0 仍硬拒（其余消费方逐一核对：
  structure_gen / 修补 / smoke 两档均只取 `!warning`）。
- 生长侧 `learning_graph/mod.rs::validate_batch`：扫描节点标题、
  `mints.canonical/aliases`、`new_aliases.name` 的 L1 形态，新增 warn
  `ng.mints.name_hint`（不阻断、不触发重裁、不拼进 `feedback`）；读数经
  `GateOutcome.name_hints` → `GrowthRunStats.name_hints` → smoke 生长读数行
  `[names: name_hints=n]` 与 OK 行 `name_hints={n}`。
- 契约 `docs/contracts/learning-structure-artifacts.md` v1.8→v1.9：§2.1/§2.2
  条款拆分、新增 §2.3 名字形态三层表（含与 learnhub 差异：growth v1 只提示、
  取消词边豁免、修补只留痕）、§6 L2 裁决位、§7 差异清单新增一行。
- f2 的提示词交卷自检句保留；机械门误杀由本票收口（f2 blueprint 留档未改）。

**测试命令与结果**：

- `CARGO_TARGET_DIR=C:/Users/test/Desktop/work/cargo-target-absorb7-y01
  cargo test -p nomifun-learning` → **792 passed, 0 failed**（含新增：L0/L1
  契约、`collect_name_suspects`、name-bundle 五资产注册/围栏/金样、裁决器
  8 例（批量/中立输入/memo/重复名/传输/解析/形状）、structure_gen 5 例
  （`求和公式` 放行、`百分数与利率的对应` 红条+理由重试、L0 不付费、
  Degraded 五档放行、memo 跨 attempt）、生长 hint 一例）。
- 新 golden 用 `LEARNING_UPDATE_GOLDEN=1` 生成后 `git status` 仅新增
  `golden/name-bundle.md`（其余四份零漂移），再以普通模式跑金样测试通过。
- `cargo fmt -p nomifun-learning -- --check` 与 `cargo fmt -p nomifun-app --
  --check` 通过；`cargo check -p nomifun-app` 通过。

**偏差**：

- 裁决调用直接走 `LearningCompleter::complete_json`（无 thinking）而非
  `complete_json_coach`（后者强制 thinking ON）——按票面「无 thinking」；
  `max_tokens` 常量住 `generation::completer`（资产参数不进 PromptSpec）。
- 修补路径未加端到端 service 测试（既有维护夹具结构未过全量 gd.*）：红/提示
  分工由契约单测锁定，service 只加读数日志。
- 真机未跑：无可用 provider/凭据，用 Scripted completer 覆盖票面红绿样；
  **真机待波次验收**（structure 档含「求和公式」正例 +「双进程与窗口」反例、
  smoke `EXIT=0`、growth 一次捆绑名 hint 可见）。
