---
id: pg0
title: 出处门 Stage 0——真机评审档案只读量翻转面（不改门）
type: task
status: open
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
