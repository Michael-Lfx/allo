---
id: u06
title: "历史压缩实验：已落库正文在历史中换摘要 + ls_inspect 可取回原文"
type: task
status: closed
lane: B
blocked_by: [u05]
part_of: ../map.md
---

# u06：不再每轮重发已落库的整节正文（高风险，缺省 off）

## 问题与证据（审计③#1，证实）
- `loop_core.rs` ~:219-238 每轮 `system + messages.clone()` 全量重发，节正文以 ToolUse 参数（`ls_set_section_body` 的 body）永留历史：一课 24 调用累计输入 687,801 字符而正文仅 2,846 字，产出/输入≈0.4%。

## 风险（为何缺省 off）
- 压缩后模型看不到自己写过的正文，可能伤到跨节一致性（练习节引用概念节的数值/例子）。故提供按需取回原文的工具能力，并以 A/B 决定是否打开。

## 实施要点
- [ ] `loop_core::run_agent_loop` 增可选压缩钩子（参数缺省 None）——**只有课时内容循环传入**，课程大纲/学习循环零变化。
- [ ] 钩子语义：每轮工具结果回灌后，把**已成功落库**的 `ls_set_section_body` / `ls_set_document` 的 body 参数替换为「（已落库：{section_key}，{n} 字。需要原文时调用 ls_inspect 并传 section_key）」；最近一次写入的那一节保留原文（保证紧邻的修改仍有上下文）。
- [ ] `ls_inspect` 增可选 `section_key`：传入时返回该节完整正文（现状只返回草稿视图）。
- [ ] 开关 `NOMI_LEARNING_LESSON_HISTORY_COMPACT`，**缺省 off**；仅当 A/B 达标才翻 on。

## 三件套
- **主指标**：每课时累计输入字符（目标降 ≥40%）、总调用数。基线同 u05（以 u05 合入后的 on 臂为基线再测本票增益）。
- **护栏（每臂 2 跑）**：content 臂判据全过；跨节一致性抽查（练习/例题数值与概念节一致）；math_verify failed 不增；节数/题数不缩水；ls_inspect 取回调用若频繁到抵消收益则判失败。
- **推翻条件**：任一护栏劣化 → 保持缺省 off，记录数据。
