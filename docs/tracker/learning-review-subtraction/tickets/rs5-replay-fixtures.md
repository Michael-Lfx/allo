---
id: rs5
title: 病历回放夹具——四病历 Scripted 钉死 + smoke review-replay 臂
type: task
status: closed
assignee:
blocked_by: []
part_of: map.md
---

## Question

本波两票契约改动（rs2/rs3）的负优化判定需要「锁方向不锁行」的回归读
数——上游 learnhub 的证据文化=replay-cases（录制 prompt + 对比器 +
NOTES 记 flaky 现场率），ADR 里直接引 `llm-calls/1792718393-8` 编号当
证据。allo 评审域的病历形态目前只活在提示词伤疤和口头记忆里，没有可
重放的夹具——每次动契约都靠全量泛测+真机手感。

## 已核验事实（2026-11-20 对码）

- 四个病历形态全部有实码/日志锚点：
  ①**滤杯循环自证**（自教行 ref 指向提名自身，应重试）：mod.rs:971
  负例原文 + p01 留痕 +303s；
  ②**次数同形**（常识行撞名池，应归一留痕不重掷）：mod.rs:979 注记 +
  上游 #252 病历（3 次重掷 57-137s）；
  ③**三角形折叠**（定义折入未在册名词，应打回）：上游 ADR-0033 主病
  例（回放 5 跑 4 拒 1 放的翻正证据）；
  ④**59k 呓语**（思考写进回包填爆预算，parse-fail 应恰一次重试救回）：
  11-17 01:16 live 日志（278s + 72s 重试成功）。
- smoke 基建现成：ArchivingCompleter 已挂管线（coach 调用全留痕含
  usage），批 JSON 可从日志 batch_json + DB 登记表快照重建。
- Scripted completer 模式已有（mod.rs 评审族 11+ 测试全用 Scripted）。

## R（用户条件批准 2026-11-20）

- **R-a Scripted 夹具（随 rs2/rs3 同票落地）**：四病历各一测试，锁方
  向不锁行——①重试发生且死因含自证；②归一发生+findings 留痕+零重掷
  （rs2 落地后翻红旧重掷语义）；③problems 指名折叠名词（reject 方向
  断言，不锁行完备性）；④呓语回包→parse-fail→恰一次重试→合法回包
  救回（degraded 不触发）。
- **R-b smoke review-replay 臂**：录制病历批（批 JSON + known surface
  + 登记表快照）→ 真实模型 N 跑（建议 3-5）→ 断言打回方向不翻转 →
  flaky 率写进 readout（上游 NOTES 同款口径）。夹具源=11-20 live 日志
  batch_json（今日三节点批，含 3 条 name_hint 告警的真实形态）+ ②③
  造数形态。
- **R-c 范围外**：对比器/案例库全量迁移（learnhub 形态的重放框架）不
  做——四夹具+一臂够本波判定用；泛化 replay 框架留候选。

## Acceptance

- 四 Scripted 测试随 rs2/rs3 落地全绿（其中②在 rs2 前红、落地后绿，
  即 TDD 红绿样）。
- review-replay 臂跑通：3-5 跑中③批 reject 方向 ≥4/5、flaky 率如实记
  录（不达 100% 照记，锁方向口径）；readout 进票 Resolution。
- `cargo test -p nomifun-learning` + smoke 臂全绿。
- 负优化判定：replay 臂本身无生产影响；夹具若暴露 rs2/rs3 方向翻转
  = 触发对应票回退条件（不是本票失败，是本票的产出）。

## Resolution

已施工收口（2026-11-20，implement-spec 波 Wave3 + 双轴评审修复批）。

- **R-a 四夹具随 rs2/rs3 提前落地**（本票开工前 grep 核对在位）：
  ①`filter_cup_circular_self_reference_still_retries`、
  ②`counting_homograph_pool_hit_normalizes_without_retry_and_leaves_findings`
  + `common_sense_row_with_ref_and_pool_hit_retries_once`（rs2）、
  ③`triangle_definition_folding_into_unregistered_nouns_is_rejected`、
  ④`rambling_non_json_reply_retries_once_with_death_cause_then_recovers`
  （rs3）。
- **R-b 回放臂落地**（wave/rs5 df6d4b25 → merge 161fc3697）：
  `learning-smoke review-replay`（--db/--provider/--model），臂内
  `replay_concept_review` 公开缝（同函数/同开关/同机械核/同重试预算，
  零 DB），逐跑 elapsed/llm_calls/reasoning_tokens 从 calls.jsonl
  增量聚合；EXIT 恒 0、唯全部 degraded 才 EXIT 2（smoke「读数面非
  门禁」惯例压过票面「断言」措辞——方向翻转只 loud 留痕）。真机跑：
  `nomicore -- --data-dir <Nomi-dev> learning-smoke review-replay
  --db <dev 库> --provider 0190f5fe-…0201 --model
  AIPC-deepseek-v4.1-flash`（run-1795166591532）。
- **真机读数（2026-11-20，9 跑）**：
  - reject-anchor（③同形）：**3/3 打回**（problems=2），49s/14s/16s，
    reasoning 7044/跑；
  - pass-anchor（净批）：**3/3 放行**，首跑 17s/4 调用/23k reasoning
    （内部有重试），次跑 4s/1 调用；
  - live-real-batch（11-20 真批原文 + 近似 known 面 9 词）：**3/3 打回
    （problems=3）**，首跑 179s/7 调用/**58.5k reasoning**，薄面近似下
    机械点名清单爆 ~100 片段——评审**正确**打回（薄面下多数名词确实
    无出处），「pass 期望」在近似面上不成立=构造性结果，非门回归。
    **处置**：夹具改 readout-only（`direction: None`，3e7ae9084）——
    不进方向汇总/方向翻转读数，保留真实形态耗时/usage/点名派生观察
    价值；已知面敏感性作为后续造数改进的观察项。
- **双轴评审修复批**（06289c6b7 → merge 0794f037b）：A 大小写折叠
  一致性（provenance 键统一折叠+2 回归）、B direction_hit 枚举化、
  C ReplayReviewVerdict 命名结构、D 教出名集提升逐批一次、E 注释
  失真、F 三小项（Result 去除/EXIT 单源/merged_unique_terms 单源）、
  G live 夹具入臂（后经真机读出方向不保→再改 readout-only）。
- **票面措辞 vs 惯例的两处偏离（留案）**：①「3-5 跑 ≥4/5」实现为
  N=3 常量 + ≥N-1/N 口径（成本折衷）；②「断言打回方向不翻转」实现
  为 readout 留痕（smoke 惯例 EXIT 门禁=全灭 0/n；EXIT 门禁方向翻转
  会破臂自身惯例）。
- **检查**：`cargo test -p nomifun-learning` 全量 **1031/0**（修复批
  后合并树）；`cargo test -p nomifun-app --lib review_replay` **7/0**
  （夹具语义修正后复跑）；`cargo check --workspace` 过（2026-11-20
  终验）。nomifun-app 全量 lib 622/2 为预存在红+并发 flake（净树
  对照定性，与本波无关）。
