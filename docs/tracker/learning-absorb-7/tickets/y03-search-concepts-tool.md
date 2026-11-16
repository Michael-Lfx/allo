---
id: y03
title: "生成面只读检索工具 search_concepts——概念登记表自查通道"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# y03: `search_concepts` 只读检索工具

**What to build:** 给课时内容生成/修复循环加一个只读工具 `search_concepts`：
模型可按需查概念是否在册、有哪些别名、有没有近名，避免只能等铸名门失败后
才看到反馈。四通道（exact / alias / nearname / fulltext）、按 concept_id
去重取最高等级、截 top-5、零命中返回 `not_found` 不抛错；严格只读，零写侧。

## 现状（2026-11-04 实码核实）

- 内容循环工具白名单 = `nomifun-ai-agent/src/lesson_content_loop.rs:883-899`
  的 9 个 `ls_*` + `verify_math`；`LoopContext` 持
  `Arc<LearningService>`（:536-544），handler 走 `one_shot_handler`。
- 登记表 `learning_concept_registry`（迁移 070:180-199；canonical 索引
  :251）与别名结构化记录（`learning_graph/alias_records.rs`）已存在。
- 近名机械已单源：`learning_graph/defnear.rs:77,95,147` 的
  `trigram_jaccard` / `name_near_form` / `stem_similarity`。
- 现在只有「预渲染注入」：教练/生长拿 `render_registry_excerpt` 的 200 行
  切片（`service/learning_graph.rs:4416-4471`）；内容面只拿本节点
  `known_names` / `concept_faces`（`lesson_draft.rs:223-242`）。模型不能按需
  查全册，撞名只能在门禁失败后知道。

## learnhub 参考（main@22d016d）

- `src-tauri/src/engine/contentgen/search_concepts.rs:65-111,176-224`：
  `ToolSpec{name:"search_concepts", parameters:{query, mode:search|detail,
  concept_id?}}`；四通道 exact/alias/nearname/fulltext；按 concept_id 去重取
  最高等级、截 top-5；零命中 `not_found=true` 不抛错；detail 返回全定义+
  全别名；登记表读失败转模型可读 Err。
- `engine/contentgen/facade.rs:315-326`：仅在「登记表非空 ∧ 本节点 teaches
  非空」时注入装配行并挂工具；plan 面并 `read_prior_content`、write 面并
  `verify_math`。
- 工具调用走既有 tool-calls 观测面（`search_concepts.rs:234-242`）。

## 实施要点

### A. 学习侧查询（DB 只读）

- [ ] `models.rs` 新增 DTO（Serialize）：
  - `ConceptSearchHit { concept_id, canonical, aliases: Vec<String>,
    definition, matched_by: "exact"|"alias"|"nearname"|"fulltext", score: f64 }`
  - `ConceptSearchResults { hits: Vec<ConceptSearchHit>, not_found: bool }`
  - `ConceptSearchMode { Search, Detail }`（serde kebab）。
- [ ] `service/learning_graph.rs` 新增 `pub async fn search_concepts(
  &self, query: &str, mode: ConceptSearchMode, concept_id: Option<&str>)
  -> Result<ConceptSearchResults, AppError>`：
  - 查 `learning_concept_registry WHERE deprecated = 0`（canonical/
    aliases_json/definition/concept_id）；`doc_version` 不匹配的别名记录
    skip + warn（沿用 `render_registry_excerpt` 口径）。
  - 通道：canonical 大小写不敏感精确 → 别名精确 → 近名（
    `defnear::name_near_form` ∪ `stem_similarity >= STEM_SIMILARITY_THRESHOLD`）
    → definition 包含 query。
  - 去重：同 concept_id 取最高等级（exact > alias > nearname > fulltext），
    同级取最高 score；截 top-5。
  - `mode=Detail`：必须带 `concept_id`；返回该概念全定义 + 全部 active
    别名（不受 top-5 限制）；不存在 → `NotFound`；缺 concept_id →
    `BadRequest`。
  - 零命中 → `hits: []`, `not_found: true`，不抛错；query 空 →
    `BadRequest`。
- [ ] 纯排序/去重逻辑抽成可单测函数（不依赖 DB），DB 只负责取行。

### B. 工具接线（ai-agent）

- [ ] `lesson_content_loop.rs` 新增 `search_concepts_tool(ctx)`：
  - `name: "search_concepts"`；description 说明「只读自查：查登记表正名/
    别名/近名/定义；不改任何东西」；schema `query`（required）、
    `mode`（enum search/detail，默认 search）、`concept_id`（detail 用）。
  - handler：解析入参 → `ctx.service.search_concepts(...)` →
    `Ok(json_compact(&results))`；错误转 `to_string()`（模型可读）。
  - 加入 `tools()`（:442）与 `lesson_content_tools`（:883）两面；**生成与
    修复都备**。
- [ ] 注入条件：学习图路径 `!ctx.context.teaches_tiers.is_empty()` 时挂
  （传统线无 teaches，不挂）；登记表为空由查询返回 `not_found` 兜底，不额外
  加计数查询。
- [ ] 图线 user 文本（`lesson_user_text`）在已知概念/档位行之后补一句：
  「可用 `search_concepts` 自查概念是否在册、别名与近名；只读，不改登记表。」
  同一句只出现一次；传统线不出现。
- [ ] 调用计数/观测沿用现有 one-shot tool 观测面；不新增表/WS 事件。

### C. 测试

- [ ] 纯函数：四通道命中、等级排序、concept_id 去重、top-5 截断、零命中
  `not_found`、空 query 400。
- [ ] DB 集成：deprecated 排除；别名 doc_version 不匹配 skip；detail 返回
  全别名；不存在 404。
- [ ] 工具：schema 枚举；handler 正常/错误回包；白名单两面都在；传统线
  （teaches 空）不出现该工具。
- [ ] 只读自证：调用前后 `learning_concept_registry` 行数/内容不变。

## 验收

- `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿。
- 真机 content 档：模型至少一次调用 `search_concepts`，回包含 `canonical`/
  `matched_by`；查一个不存在名返回 `not_found:true` 且循环不中断；
  smoke `EXIT=0`。真机臂串行。

## 非目标

- 写侧登记表工具（铸名/改别名/建边一律不做）。
- HTTP 路由/前端查询面（纯生成面工具）。
- 传统课程线（无 teaches 概念体系）。
- 跨课程图 join（登记表是全局只读面，既有 ADR-0010 边界不变）。

## 风险

- 登记表规模小，全表扫描 + 内存过滤可接受；别名在 `aliases_json` 内，需
  每次解析，补一条规模上限/超时保护（沿用现有查询超时）。
- 工具增加会改变生成分布与调用轮数；用工具描述与一句装配行控制，不额外
  加硬门。
- 近名阈值必须复用 `defnear` 单源，不得新写第二套相似度。

## Resolution（2026-11-04）

**评审修复（2026-11-04）**

- search 取行改为有界查询 `registry_entries_bounded(limit)`（`ORDER BY
  canonical LIMIT ?`，硬上限 500；命中上限 `tracing::warn!` 留痕但读侧不
  报错）。共享 `registry_entries()` 保持原无界语义（渲染/扩邻需要全集）。
- detail 改为 `registry_entry_by_id`（`WHERE concept_id = ?`）精确查，不受
  search 上限影响——截断之外的登记行仍可按 id 取回。
- 新增 DB 测试 `search_read_is_bounded_while_detail_stays_exact`：小上限
  验证 canonical 序截断、截断行集内的 concept_id 去重，以及 detail 取回
  截断外行。

**实现点**

- 学习侧 DTO（`models.rs`）：`ConceptSearchHit { concept_id, canonical,
  aliases, definition, matched_by, score }`、`ConceptSearchResults { hits,
  not_found }`、`ConceptSearchMode { Search, Detail }`（serde kebab）；经
  `lib.rs` crate 根再导出（ai-agent 工具面直接引用 `ConceptSearchMode`）。
- 纯函数面（新增 `learning_graph/concept_search.rs`）：四通道
  `search_hits(registry, query)`——canonical 大小写不敏感精确 → 活跃别名
  精确 → 近名（`defnear::name_near_form` ∪ `stem_similarity >=
  STEM_SIMILARITY_THRESHOLD`，名字池 = canonical ∪ 活跃别名）→ definition
  包含 query；按 concept_id 去重取最高等级（同级取最高 score），等级 →
  score 降序 → concept_id 升序，截 `SEARCH_TOP_K = 5`；零命中
  `not_found: true`。`detail_hit(entry)` 返回完整定义 + 全部活跃别名。
  零新相似度实现，近名机械全部来自 `defnear`。
- 服务面（`service/learning_graph.rs`）：`pub async fn search_concepts(
  query, mode, concept_id)`——空 query → `BadRequest`；search 走纯函数；
  detail 缺/空 concept_id → `BadRequest`、查无 → `NotFound`、命中 → 单个
  full hit（`matched_by = "exact"`、score 1.0）。取行复用既有
  `registry_entries()` 单源（`deprecated = 0`、别名池按行级 `doc_version`
  解码、未知代际降级为空池 + warn，与 `render_registry_excerpt` 同口径）；
  DB 只取行，排序/去重全在纯函数。
- ai-agent（`lesson_content_loop.rs`）：新增 `search_concepts_tool(ctx)`——
  description 明示「只读自查，不改登记表（不铸名/改别名/建边）」；schema
  `query`（required）、`mode`（enum search/detail，default search）、
  `concept_id`；handler 解析入参 → `service.search_concepts(...)` →
  `json_compact`，零命中 `Ok(not_found: true)`，入参/登记表错误转
  `to_string()` 文本 Err。挂载条件 `graph_teaches_concepts` = 图线 ∧
  `teaches_tiers` 非空；`lesson_content_tools` 对生成/修复两面同一装配点
  挂载（`tools()` 本就是把两面委托给它的唯一入口）。`lesson_user_text` 同
  一条件在已知概念/档位段之后追加一句
  「可用 `search_concepts` 自查概念是否在册、别名与近名；只读，不改登记
  表。」（常量 `SEARCH_CONCEPTS_HINT`，只出现一次；传统线与零 teaches
  节点不出现）。
- 严格只读：服务只发 SELECT；无 HTTP 路由、无新表/WS 事件、无写侧工具。

**测试**

- 新增纯函数测试 11 条（`concept_search.rs::tests`）：四通道（大小写不
  敏感）、跨概念等级排序（exact > alias）、近名两条分支
  （name_near_form / stem_similarity 阈值）、同级 score 降序、同概念去重
  取最高级、top-5 截断、零命中 `not_found`、detail 全定义 + 全别名。
- 新增服务级 DB 测试 4 条（`service/concept_search_tests.rs`）：
  deprecated 排除（search + detail 404）、退役别名不占池、未知
  `doc_version` 别名池降空 + warn 口径、detail 六别名不受 top-5 限制、
  空 query/缺 concept_id → 400、查无 id → 404、search 截 top-5、只读自证
  （search/detail 前后登记表行数、内容与 `updated_at` 逐字不变）。
- 新增 ai-agent 测试 4 条（`lesson_content_loop.rs::tests`）：挂载只在
  「图线 ∧ teaches 非空」（生成/修复两面都挂；零 teaches 与传统线都不
  挂）、schema 枚举/required/concept_id、handler 命中（canonical/
  matched_by）/别名命中/零命中 Ok/detail 六别名/缺参缺 id 非法 mode 的
  可读 Err、工具调用前后登记表快照逐字不变。
- 命令（独占 target）：
  `CARGO_TARGET_DIR=C:/Users/test/Desktop/work/cargo-target-absorb7-y03
  cargo test -p nomifun-learning -p nomifun-ai-agent` → **EXIT=0**；
  `nomifun-learning` lib 792 passed / 0 failed；`nomifun-ai-agent` lib
  1132 passed / 0 failed / 3 ignored，全部集成测试二进制 ok。
- `cargo fmt -p nomifun-learning -p nomifun-ai-agent -- --check` 通过。
- 集成分支尖端仍为 `9092c7d9a`（本票期间无新提交），
  `git merge feat/learning-absorb-7` 为 no-op；合并尝试后复跑同一命令
  **EXIT=0**（learning 792 / ai-agent 1132 + 3 ignored）。

**偏差 / 未做项**

- `tools()` 在当前实码是 `lesson_content_tools(ctx, repair_face)` 的一行
  委托（无第二份白名单），故工具只在 `lesson_content_tools` 单点加入即
  覆盖生成/修复两面——满足票面「两面都备」，未额外造第二个列表。
- 取行未新写查询：既有 `registry_entries()` 已是「deprecated=0 + 别名
  doc_version 解码」的单源，直接复用（票面「DB 只负责取行」不变）。
- `matched_by` 的取值域仍按 DTO 注释四种：detail 按 concept_id 精确取行
  记 `exact`（score 1.0），未新增第五种枚举值。
- search 命中返回完整 `definition`（DTO 只有 `definition` 字段，无摘要
  字段）；detail 与 search 的差别在「按 id 取行、不受 top-5 限制」。
- 真机 content 档冒烟（模型至少一次调用 + 不存在名 `not_found` + smoke
  `EXIT=0`）未在本票执行：本 worktree 无 provider 凭据，留给 merger 的
  串行真机验收（与 y02 同处理）。
- 全量套件并行时偶发两条与本票无关的既有抖动各一次
  （`agent_trace::hub::tests::drop_conversation_observations_deletes_jsonl`、
  `manager::openclaw::connection::tests::dropping_connect_future_closes_socket`）
  ——单跑均绿，要求的合并命令复跑 EXIT=0。
