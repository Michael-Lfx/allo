---
id: u05
title: "verify_math 批量（claims 数组，向后兼容）"
type: task
status: closed
lane: B
blocked_by: []
part_of: ../map.md
---

# u05：一次核验整节的可算量

## 问题与证据（审计③#2，证实）
- verify_math 工具 schema 只收单个 claim（`lesson_content_loop.rs` ~:1366-1379），而 R14 要求「凡可算量必实算」——负样本 run-1793860620962 的 25 轮里 20 轮是核验调用，另一课 16 轮里 10 轮；每轮又全量重发历史（放大成本）。

## 实施要点
- [ ] 工具入参在原单 claim 形状之外接受 `claims: [ {…}, … ]`（向后兼容：单 claim 照旧可用）；返回逐条结果列表（每条 pass/fail + 实算值 + 修正指引，沿用现单条文案）；上限（如 ≤12 条/次）与超限报错可读。
- [ ] 工具描述与 R14 条款（on 臂）补「一节写完后把本节全部可算量一次性提交核验」。
- [ ] 落账/读数（math_verify calls/passed/failed/unresolved）按**claim 计数**不变（批量不改变核验覆盖口径），另加 `math_verify_batches` 读数。
- [ ] 开关 `NOMI_LEARNING_VERIFY_BATCH`（缺省 on 待 A/B）：off 臂工具 schema/描述/条款逐字节等于基线。

## 三件套
- **主指标**：每课时 verify_math 工具调用次数、总 LLM 调用数、累计输入字符。基线：run-1793861260083 llm_calls=16 / input=605,189；run-1793860620962 llm_calls=24 / input=687,801、核验 20 调。
- **护栏**：被核验的 claim 总数不下降（覆盖不缩水）；math 失败不增；content 臂判据全过。
- **推翻条件**：on 臂 claim 覆盖数下降 >20% 或 math_verify failed 上升 → 默认 off。
