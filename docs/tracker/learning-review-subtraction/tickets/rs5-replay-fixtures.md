---
id: rs5
title: 病历回放夹具——四病历 Scripted 钉死 + smoke review-replay 臂
type: task
status: open
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
