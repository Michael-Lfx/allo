# o1: growth accept 路径对称日志 + LLM 逐调用计时

## Status

已关票（2026-11-16 施工收口，feat/learn1116）。

## 已核验事实（2026-11-16 对码）

- **accept 静默**：reject 有 WARN（`service/learning_graph.rs:2623`
  `growth batch rejected by concept review`，带 attempt/problem_count/
  report），通过路径零日志——2026-11-16 病历里 attempt-3 通过后无任何
  INFO，确认只能查 DB `learning_growth_batches.status='applied'`。
- **逐调用零计时**：LLM 调用出口集中 `complete_json_coach`
  （`generation/completer.rs:131`，480s 超时），成功路径无日志，只有
  超时 WARN（`json coach call timed out`）。`observation/learning-tasks.jsonl`
  只有批级聚合（llm_calls/input_chars/output_chars/elapsed_ms），单次
  调用耗时只能靠日志行差值反推。
- **调用点面**：`complete_json_coach` 非 test 调用点 10 处 6 文件——
  `learning_graph/mod.rs:828`（coach draft）/`:873`（coach redraft）/
  `:1774`（concept review）、`structure_gen.rs`×2、`structure_ops.rs`×1、
  `mounting.rs`×1、`service/learning_graph.rs`×2。
- **病历读数**（方差证据）：同形状调用 2026-11-16 一天内 attempt-2
  redraft 7.2s vs attempt-3 redraft 1m57s vs attempt-1 proposal 4m38s；
  growth#1 同型 proposal 仅 62s。无逐调用计时则这类方差无法归因。

## Terms

- **R1（accept 对称日志）**：growth 批通过补一行 INFO：course、attempt
  序号、累计 elapsed、nodes/mints 数——与 reject WARN 对称。不打
  batch_json 正文（数字与标签即可；reject 现有 report 字段保留现状）。
- **R2（逐调用计时）**：每次 LLM 调用一行 INFO：stage 标签
  （draft/redraft/review/structure/mount/…）、耗时 ms、system/user/
  output 字符数。实现自选：`complete_json_coach` 加 stage 参数（10 处
  调用点同形小改）或调用点计时包装；不改变调用语义、重试环与超时。
- **R3（不做）**：不动 thinking/effort（归候选②，等 o2 归因数据）；
  不动 learning-tasks.jsonl 聚合面；新日志零 prompt/回包正文（隐私红线；
  现有 `coach proposed growth batch` 打 batch_json 属现状，不扩散）。

## Acceptance

- `cargo test -p nomifun-learning` 绿。
- 真机跑一次 growth：从 nomicore 日志直接读出每次调用的 stage/耗时/
  字符数与 accept 行（attempt 序号+耗时），无需查 DB 或行差值反推。
- 新增日志行 grep 自查零正文内容（只有数字、标签、id）。

## Resolution

已施工收口（2026-11-16，feat/learn1116）。

- R1（accept 对称行）：`run_growth` 带运行级 `Instant`，通过臂落
  `learning graph growth batch accepted`（course/elapsed_ms/attempts/
  nodes/ready/idle，零正文）；真失败臂对称落 `learning graph growth run
  failed`（course/elapsed_ms；cancelled 臂不记——用户取消不是诊断对象）。
  `GrowthApplied` 增 `attempts_used` 字段（`growth_body` 的预算轮消耗数
  回填，apply_batch 内置 0 由调用侧覆盖）。
- R2（逐调用计时）：`complete_json_coach` 增 `stage: &'static str` 参数，
  每次 LLM 尝试恰一行 INFO（`learning coach llm call done`：stage/
  elapsed_ms/system_chars/user_chars/output_chars；失败臂同形状带
  `ok=false`）——9 个非 test 调用点全部挂标：growth_draft /
  growth_redraft / growth_review / structure_terrain / structure_artifact /
  structure_repair / structure_mount / basis_declare / reuse_ruling。
  480s 超时 WARN 保留原样（成功路径新增计时行）。
- R3（不做）如实执行：thinking/effort 未动（归候选②，等 o2 归因数据）；
  learning-tasks.jsonl 聚合面未动；新日志零 prompt/回包正文。
- 验证：`cargo test -p nomifun-learning` 964 绿（含新增
  `call_archive_switch_defaults_off_and_requires_explicit_on`——o2 票的
  开关测试，落同 crate）；`cargo check` 三 crate 绿；`cargo fmt` 过。
- 真机复验点：下一次 growth 后从日志直接读出逐调用耗时与 accept 行
  （对照 2026-11-16 病历的行差值反推）。
