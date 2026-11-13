---
id: t1
title: "全量清点 + 分诊：core/prerequisite/practice/mixed/noise/duplicate/uncertain，逐项理由"
type: task
status: closed
assignee: zhangyj
blocked_by: [d1]
part_of: ../map.md
---

# t1: 全量清点 + 分诊

**What to build:** 在传统知识库建课的大纲阶段之前，新增**全量清点 + 分诊**，替代“直接 40 文件采样”作为结构发现的第一层。目标不是“读完全部文件”，而是“把全部文件分对类”：哪些要教、哪些是前置背景、哪些是练习、哪些是混合、哪些是噪声/重复、哪些判不准。分诊结果必须逐项带理由，不确定的保守保留，绝不静默丢内容。

**Blocked by:** [d1 设计定稿](d1-design-and-code-state.md)。

**Status:** ready-for-agent

## Grill 第一轮/第二轮定案（2026-10-21）

- **单位**：文件级只定**角色**（core/prerequisite/practice/mixed/noise/duplicate/uncertain）；章节/主题级覆盖归 t2（heading/anchor 为单位）。section 级噪声只处理明显可分离的推广尾段/出版页/导航页，不做全量 section 级 LLM 分诊。
- **范围**：v1 只覆盖 Markdown 文档面；翻译副本/同 hash 重复按规则自动排除并给理由；PDF/Word/图片后续单独立票。
- **失败策略**：`uncertain` 保留为候选，不静默丢；无 LLM/completer 时规则分诊降级（非噪声进 core/prereq，`degraded=true`），建课不阻断；只有零章节/零 core 硬失败（由 t2/t3 判定）。
- **代码落点**：清点/分诊编排与提示词住 `nomifun-learning`；`KnowledgeService` 只做文件/检索 IO，`LearningCompleter` 做 LLM。
- **成本门**：清点/规则零 LLM；LLM 分诊按 40/批；>40 文件或预估 >20 次调用时按 t5 的估算/确认门处理（t1 只提供读数）。
- **混合文件（Q9）**：新增 `mixed` 角色；有任一相关段就不整文件排除，哪些 heading/anchor 进课程由 t2 的章节/主题覆盖决定；噪声段显式 skipped + anchor。
- **重复/翻译 canonical（Q12）**：规则选 canonical（非翻译后缀原文优先、README/字符占比判语言、同语言最新/最高质量）；其余标 `duplicate_of` 自动排除并给理由；v1 不做逐项用户确认。

## 输入/输出

**输入**：`GenerateCourseRequest` 的知识库 id 列表 + `goal`；`KnowledgeService`。

**输出**（新类型，命名可实施时定）：

- `KbInventory`：
  - `files: Vec<KbInventoryFile { rel_path, kb_id, size, modified_at, content_hash, headings, preview, kind }>`
  - `total_files`、`total_bytes`、`skipped_hidden`。
- `KbTriageReport`：
  - `files: Vec<KbTriageFile { rel_path, kb_id, role, confidence, reason, duplicate_of? }>`
  - `counts { core, prerequisite, practice, mixed, noise, duplicate, uncertain }`
  - `excluded { path, role, reason }[]`
  - `duplicates { path, duplicate_of, canonical_reason }[]`
  - `degraded: bool`（无 LLM/LLM 失败时规则降级）
  - `warnings: Vec<String>`。

## 实施要点

- [ ] **清点**：对每个来源库调 `KnowledgeService::list_files`（现走 `list_md_files`，v1 只覆盖 Markdown 文档面），并用 `read_file` 读正文；记录 `rel_path/size/modified_at/content_hash/H1-H3 标题/开头 preview`。多库路径带 `{库名}/rel` 命名空间，沿用 t4 口径。
- [ ] **规则预分类**（高置信、零 LLM）：
  - 元数据：`license/licence/contributing/code_of_conduct/security/changelog/authors/maintainers/pull_request_template/issue_template/support/faq/citation/codeowners` → `noise`；
  - 空文件/纯导航页（例如只有链接列表、无正文）→ `noise`；
  - 翻译布局检测（microsoft/parallel/suffix）：翻译副本从原文候选分流；成对翻译记录 `duplicate_of`；孤儿翻译保守当原文；
  - 同 `content_hash` → `duplicate`（按 canonical 规则保留一份；其余 `duplicate_of` + `canonical_reason`）；
  - canonical 规则（Q12）：非翻译后缀的原文优先；README/字符占比判原文语言；同语言选最新/最高质量（mtime/大小/标题完整度可作 tie-break）；规则无法判定 → `uncertain`，不自动排除；
  - 近似重复（可选 v1：归一化空白/标题后 hash；不做 embedding）。
- [ ] **LLM 分诊**：剩余文件按 ≤40/批，输入 `goal + README/目录树 + 每文件标题/preview/类型`；输出每个文件的 `role + confidence + reason`。角色：
  - `core`：本课程要教的内容；
  - `prerequisite`：目标需要但不必单独成课的前置背景；
  - `practice`：练习/lab/代码；
  - `mixed`：文件既有相关正文、也有明显无关段（会议记录/推广/旧章节）；**不整文件排除**，交给 t2 的 section/topic 覆盖决定哪些段进课程；
  - `noise`：与目标无关、元数据、推广页、空导航；
  - `duplicate`：重复/翻译副本/旧版本；
  - `uncertain`：判不准。
- [ ] **不确定处理**：`uncertain` 默认进入后续结构发现的候选集（保守保留），并在报告里标记；**禁止静默丢弃**。每个 `noise/duplicate` 必须有 reason。
- [ ] **降级**：无 LLM/completer 或分诊调用失败 → 规则分诊兜底：非噪声全部 `core`（翻译/重复仍按规则标记），`degraded=true`，建课不阻断；UI/日志披露。
- [ ] **批内截断**：复用/抽出结构化输出“批拆半”工具（与 t3 共用；图生长也可复用，见 g1）；单文件仍失败 → 该文件 `uncertain` + reason，不丢。
- [ ] **并发/预算**：分诊 LLM 调用限制并发（建议 ≤4），每批 token 上限；清点只读一次正文（后续结构发现复用 inventory 的 preview，不重复读全库）。
- [ ] **不读正文做结构结论**：t1 只分类，不产出章节/课程结构；结构在 t2。

## 非目标

- 不落库/不改迁移（t4）。
- 不做 PDF/Word/图片 OCR（v1 Markdown 文档面；后续单独立票）。
- 不做 embedding 语义重复检测（v1 hash/规则；后续增强）。
- 不把 `uncertain` 合并进 `core` 冒充确定。

## 验收

- **脏库单测**：构造一个混合库（core 章节 + 收据/会议记录/剪藏 + LICENSE/配置 + 翻译副本 + 同 hash 重复 + 缺失标题 + 空导航页 + 一个 mixed 文件），断言：
  - 所有文件都出现在报告中（总数守恒）；
  - core 章节不被误判为 noise；
  - mixed 文件不被整文件排除，且进入 t2 的候选；
  - noise/duplicate 全部有 reason；
  - duplicate 有 `duplicate_of` + `canonical_reason`；翻译副本不重复成课；
  - `uncertain` 保留并可见。
- **canonical 单测**：原文 + 翻译 + 旧版本同时存在时，规则选原文/最新版；无法判定时进 `uncertain`，不自动排除。
- **降级单测**：无 completer 时 `degraded=true`，非噪声全部进入 core/prereq，建课不失败。
- **批拆半单测**：模拟 40 文件批 JSON 截断，断言拆半后仍覆盖全部文件；单文件失败 → `uncertain`。
- **多库**：两个库、路径命名空间正确；已删库 id 不读。
- `cargo test -p nomifun-learning` 全绿；`cargo check -p nomifun-learning` 干净。

## Resolution（2026-10-21 关票）

**交付**（分支 `learn-v2/t1-inventory-triage`，4 提交 `880c248ad..4d7bdb227`，已并入集成分支）：

- 编排入口 `run_kb_triage(knowledge, kb_ids, goal, completer) -> (KbInventory, KbTriageReport)`，住 `generation/triage.rs`（~1200 行，类型+编排+规则+中文提示词常量），lib.rs 公开导出。
- 规则预分类零 LLM：元数据词干/空文件/纯导航页→noise；翻译布局（`guide.zh-CN.md` 后缀 + `en/` 目录统一为去语言键分组，孤儿翻译保守当原文）；同 sha256→duplicate；Q12 canonical 顺序 = 非翻译后缀原文 → README → 目标语言（CJK 占比）→ 最新 mtime → 全平手双 uncertain 不排除。
- LLM 分诊 ≤40/批、并发 ≤4（buffer_unordered）；单文件解析失败→uncertain+理由；无 completer/调用失败→规则降级 `degraded=true` 不阻断；清点正文只读一次。
- **structured_batch 新写**于 `generation/structured_batch.rs`（pub(crate)，t3/g1 复用）：基于 `parser::parse_json_object` 的驱动层——整批尝试 → Call 错误（不拆半）/ Parse 错误（拆半递归）→ 部分覆盖只补缺失子集 → 单文件兜底 → 逐条目结果（输入序、总数守恒）。对象安全 `BatchAttempt` trait。
- 测试：`cargo test -p nomifun-learning` **363 passed / 0 failed**（基线 344 + 新增 19：triage 14 + structured_batch 5）；票面验收 6 组全覆盖（脏库守恒/mixed 进候选/排除有理由/duplicate_of+canonical_reason/降级两路/40 文件截断拆半 7 次调用全覆盖/多库命名空间+删库跳过）。

**实施偏差**（不违背票面）：

1. `structured_batch` 用 trait 驱动而非闭包泛型；补充 Call/Parse 失败二分契约（调用层失败不拆半）。
2. `KbInventoryFile.rel_path` = 引用路径（多库 `{库名}/rel`、单库裸路径，与采样/SourceSpan 口径一致）；另加 `bases`（逐库命名空间披露）与 `scan_scope`（markdown）两个稳定字段；`skipped_hidden` 恒 0（服务端过滤 machinery 目录，字段保留给 t4 形状稳定）。
3. 错误一律 `AppError`：零可读 markdown → BadRequest；缺库跳过+warning（票面 error_type/needs_reindex 形态在代码中不存在）。

## 备注

- `KnowledgeService::list_files` 只列 Markdown；如果用户库里有非 md，v1 在报告中如实披露“仅扫描 Markdown 文档”，不静默忽略。
- 分诊是“结构发现”的前置，不是最终课程内容；不要在这一步让模型写章节标题/课时。
- 所有排除项都要能回答“为什么没进课程”；这是脏库场景的信任基础。
