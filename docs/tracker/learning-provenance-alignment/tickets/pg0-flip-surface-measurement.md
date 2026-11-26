---
id: pg0
title: 出处门 Stage 0——真机评审档案只读量翻转面（不改门）
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

本仓的 `term_provenance` 机械域仍是 learnhub #169/ADR-0033 的 v1 移植
（四值、名池 = 已教账 ∪ 登记表活跃名 ∪ 本批名）。隔壁 2026-11-26 出处门
（learnhub #287–#293）把同一套 v1 的四个洞量成了真机读数；本仓是否同病、
病多大，没有本仓数据。先把「按新规则会翻红多少行」量出来，再决定
enforce 档与是否值得上 Stage 2。

本票**只读**：不改提示词、不改 schema、不改机械核、不跑 LLM。

## 已核验事实

- 本仓评审档案已够用：`ArchivingCompleter` 的 `calls.jsonl` 行含
  `user`（评审当时实际收到的完整输入：批次 JSON + 已教概念账 + 登记表
  切片 + 候选集）与 `response`（原始评审回包，含 `term_provenance`）——
  `crates/backend/nomifun-ai-agent/src/archiving_completer.rs:139-151`。
  dev 渠道 `NOMI_LEARNING_LIVE_ARCHIVE` 缺省开，落
  `observation/learning-live/<YYYY-MM-DD>/calls.jsonl`
  （`crates/backend/nomifun-app/src/services.rs:2820-2826`）。
- 真机档案重放入口已存在：`learning-smoke review-replay --replay-archive`
  （`crates/backend/nomifun-app/src/commands/learning_smoke.rs:5660-5730`），
  但它是「重发模型」；本票要的是零 LLM 的机械分类，不能复用该子命令。
- 本仓可从 `user` 原文解析出批次 JSON（`【待审批次】\n{batch_json}`，
  `learning_graph/mod.rs:1840-1845`）与已教概念账文本（块 6，
  `mod.rs:800-810` 附近）；`response` 直接是 JSON。
- 本仓 DB 路径与只读查法已有先例（bun:sqlite readonly 直查
  `flowy-backend.db`，见观察文档与既有巡检记录）；脚本只能读，不能写。

## 量什么（四类，逐行判）

1. **在册未教（`已教` ref 只命中登记表）**：ref 不在档案的已教概念账
   文本里，但在当前库 `learning_concept_registry` 活跃名/别名里。
   口径限制：登记表是当前态，档案是当时态；结果记「下界/上界」两列
   （能确认当时在册的才算下界）。
2. **term ≠ ref（父/整体/描述冒充）**：`已教/自教` 行的 `term` 去掉
   尾随括注后不等于 `ref`，且两者不是同一概念的活跃别名。
3. **兄弟供给**：从 `user` 里的批次 JSON 取 `nodes[].teaches`；对行
   `concept`，取「教该概念的节点」的 teaches 交集；`自教` 行的 ref 不在
   交集但在全批并集 = 兄弟供给。
4. **跨批翻转**：按 `(concept, term)` 分组，统计 source 序列出现 ≥2 种
   的对数；Top 20 列 concept/term/序列（正文截断，不贴全文）。

另附：纸面提名（`mints/new_aliases` 无任何节点 teaches）行数；按
「已教账为空 / 非空」分档（空账 = 纯零首批，是 report-only 的候选面）。

## 方法（不入生产代码）

- 一次性 Bun 脚本放 `docs/tracker/learning-provenance-alignment/assets/`，
  读 `calls.jsonl` + 只读 SQLite；输出 markdown 表到 stdout（或 `assets/`
  下读数文件）。
- 输入参数：`--archive <calls.jsonl>`、`--db <flowy-backend.db>`、
  `--course <id>`（可选，按课程分档）。
- 不调模型、不写库、不改课程数据；脚本不 import 生产 Rust。
- 读数口径与不确定性写在脚本文档头：当前登记表 vs 当时登记表、
  模型自报 vs 机械可证、样本量/课程数。

## 影响面（2026-11-26 对码）

### 新增面（只有本票 assets）

- `docs/tracker/learning-provenance-alignment/assets/`：一次性 Bun 脚本
  + 读数表（或脚本 stdout）。**不入生产代码、不入 `scripts/`**。
- 只读输入：
  - `calls.jsonl`：行形状见 `archiving_completer.rs:139-151`
    （`system`/`user`/`response`/`usage`/`elapsed_ms`）；
  - `learning_concept_registry`（只读 SQLite，用于「在册」判定）；
  - `learning_lesson_concepts` + `learning_lessons` + `learning_modules`
    + `learning_courses`（只读，用于 taught/declared 分档；也可只从
    archive 的已教账文本读）。
- 零生产代码、零 DB 写、零迁移、零 UI、零 prompt/schema 改动。

### 测试面

- 无单测；验收 = 同一档案复跑读数一致 + `git status` 只出现 assets
  与票面。
- 若脚本需要夹具：只放脱敏后的 2–3 行 `calls.jsonl` 片段（或合成
  行），不提交原始全量档案。

### 数据/隐私

- `calls.jsonl` 含完整 prompt/回包（可能含课程内容）；脚本只输出
  计数、比例与截断示例（≤120 字符），不把原文写进票面/仓库。
- 脚本以 `readonly` 打开 DB，不触碰 WAL 写路径。

### 风险

- **当前登记表 ≠ 当时登记表**：在册判定有上下界；读数表标口径，
  不把近似值写成精确值。
- **样本偏差**：档案可能集中在少数课程/模型；按课程与空账/非空账
  分档，样本量一并列出。
- **零 LLM 的边界**：本票只量「模型当时写的表」在机械规则下的翻转，
  不重跑模型；重跑是 pg1 之后的回放对照。


## Acceptance

- 同一档案复跑两次读数一致；脚本零写（DB 以 readonly 打开）。
- 产出表：总行数、四值分布、四类违规行数与占 `已教`/`自教` 的比例、
  翻转对数、按空账/非空账分档、Top 例子（≤20 行）。
- 结论回答两问：①本仓是否需要 Stage 1 的 enforce（还是 report-only
  起步）；②翻转面是否大到值得开 Stage 2 账本（判据由本票读数给出，
  建议阈值在票面评论里定，不在实现里写死）。
- 不改任何生产文件（`git status` 只应出现本票 assets 与票面）。

## Out of scope

- 任何门/提示词/schema 改动（pg1/pg2 的事）。
- Stage 2 账本/derivation 的实现（只在读数里回答要不要）。
- Stage 3 primitive/ostensive（本仓无链接器，见 map 的非目标）。
- 用 LLM 重跑评审（档案重放是另一条臂，本票零 LLM）。

## Resolution（2026-11-26 真机档案读数）

脚本：`assets/pg0-provenance-dryrun.mjs`；读数：
`assets/pg0-readout-2026-11-26.md`。输入 = `learning-live` 3 天 +
`learning-smoke` 8 个 run，共 16 个 `calls.jsonl`；评审调用 64 次 /
673 行（live 11 次 / 84 行，smoke 53 次 / 589 行）。

### 读数

| 范围 | 评审调用 | 表行 | 已教行 | 自教行 | 在册未教 | 直接出处违规 | 兄弟供给 | 翻转对 | 纸面提名 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| live | 11 | 84 | 30 | 9 | 0 (0.0%) | 29 (74.4%) | 7 (77.8%) | 0 | 0 |
| smoke | 53 | 589 | 347 | 0 | 1 (0.3%) | 215 (62.0%) | 0 (—) | 0 | 0 |
| 合计 | 64 | 673 | 377 | 9 | 1 (0.3%) | 244 (63.2%) | 7 (77.8%) | 0 | 0 |
| 末轮合计 | 24 | 235 | 131 | 2 | 0 (0.0%) | 85 (63.9%) | 0 (0.0%) | 0 | 0 |

Top 直出例子（截断）：`垃圾回收 | term=堆对象 | ref=堆分配`、
`悬空引用 | term=存储位置 | ref=值存储位置`、
`引用计数 | term=引用 | ref=悬空引用`、
`原子操作 | term=读-改-写 | ref=赋值语句`。Top 兄弟例子：
`引用计数 | term=引用数量 | ref=引用计数`（ref 在全批并集但不在
「教该 concept 的节点」交集）。

### 结论

1. **直接出处是主翻转面**：244/386（63.2%）的已教/自教行 term 与 ref
   不是同一概念；末轮（近似实际接受稿）仍 85/133（63.9%）。这不是
   边缘瑕疵，是 v1 名池「只核 ref 可解析」的必然形态。**不能直接
   enforce**：会把大多数批次打回，重试预算耗尽。pg1 采用**分规则
   执法档**：`已教=covered` enforce（本样本 0.3%，低风险）；直接出处
   + 自教同节点先 report-only（findings/读数），提示词改写后做一次
   小样本 LLM 回放校准，再翻 enforce。
2. **在册未教在本样本很罕见**（1/377，0.3%）：登记表只有 86 个活跃名，
   而档案里的已教账很大（跨课程账），所以「在册≠已知」的爆炸半径小；
   但语义仍与 CONTEXT 冲突，且低风险，可 enforce。
3. **兄弟供给**：自教行只有 9 行，其中 7 行是兄弟供给（77.8%）；
   绝对量小，但形态真实（smoke 0 行自教 = 该样本没覆盖）。先
   report-only，避免小样本下把首批打回；校准后翻 enforce。
4. **翻转对 0、纸面提名 0**：64 次调用里没有观察到跨批 source 翻转，
   本样本**不足以支持 Stage 2 账本**。Stage 2 继续暂缓；等 live 档案
   累积到至少数百次评审调用再判（判据：同一 `(concept, term)` 跨批
   出现 ≥2 种 source 的对数 ≥10，或账本能显著缩短主评审逐词段）。

### 口径限制

- live 样本只有 11 次调用 / 84 行；smoke 批次可能是合成/干净批，代表性
  有限。读数按「风险先验」用，不当精确总体。
- 「在册」用当前登记表近似当时登记表（当前 86 个活跃名，可能已删/
  改名）；在册未教是下界。
- 直接出处按旧提示词下的模型输出测量；pg1 改写提示词后模型可能显著
  改善，必须用回放校准再决定 enforce。
