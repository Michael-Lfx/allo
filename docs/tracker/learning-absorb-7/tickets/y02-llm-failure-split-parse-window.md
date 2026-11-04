---
id: y02
title: "LLM 失败分流——parse 失败窗口 + 传输失败不占重试预算（学习模块内）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# y02: LLM 失败分流（学习模块内）

**What to build:** 把「模型没收到」（传输/超时/网关）与「收到但不可用」
（非法 JSON / 契约违约）在学习侧彻底分开：parse 失败带 ±120 字符的出错窗口，
传输失败不再被当成可修正反馈注入重试环、不再烧契约重试预算。**v1 只改学习
模块**：不改 `nomifun-common::AppError`、不改 `nomifun-ai-agent`、不改
provider 错误映射；分类只用学习侧已能看到的 `AppError` 变体。若实施中发现
必须动公共代码，先停手、记偏差、回报裁决。

## 现状（2026-11-04 实码核实）

- `generation/parser.rs:137` `parse_json_object`：失败只保留
  `invalid JSON: {serde error}` 字符串；`reply_diagnostic`（:206-221）只有
  长度/括号平衡/头 120 字符——没有行列定位、没有出错窗口；中文正文下定位
  等于不可用。
- `learning_graph/structure_gen.rs:195` `attempt_once`：两处 LLM 调用
  `.map_err(|error| error.to_string())`；`:66` 外层环把任何错误串当契约报告
  回灌并重试 3 轮——传输失败也烧满 3 轮。
- `learning_graph/mounting.rs:514`、`structure_ops.rs:702`：同样
  `Err(AppError)` → `last_was_llm = true` + `continue`，仍占轮次（虽不写反馈）。
- `service/learning_graph.rs:1783` 生长环：`AttemptFail::generation(error
  .to_string())` 把 LLM 错误并入反馈/早停指纹。
- `service/open_judge.rs:191` 判分环：传输失败与校验失败同面，且每次都写
  `retry_feedback`（:263-266）——传输错误被塞进下一轮反馈槽。
- `service/lesson.rs:721` 课时环：已有两桶分类（`is_retryable_lesson_error`）
  与专用「瞬断重试」反馈，**这是刻意设计，本票不改其重试预算**；只享受
  parse 窗口。
- 上游现状（本票不动，只记触发条件）：`nomifun-ai-agent/src/learning_completer.rs`
  对任意 `BadGateway` 盲重试一次；`factory/provider_config.rs:1039` 把
  `ProviderError` 压成 `BadGateway`（401/402/额度同形）。

## learnhub 参考（main@22d016d）

- `src-tauri/src/infra/llm.rs:27-76,199`：`LlmErrorKind{Transport,Parse}` +
  `raw_output`；`PERMANENT_MARKER` 致命豁免。
- `src-tauri/src/infra/llm_retry.rs:59-153`：`parse_feedback` 取 serde
  `line/column`，`offset_of_line_col` 按行首字节 + 列定位并退到字符边界，
  `failure_window` ±120 字符；`RetryExecutor` 仅传输类退避 1s/3s×2。
- 七个引擎 facade（如 `engine/contentgen/facade.rs:648,778`、
  `decomposition/facade.rs:192`）：重试环首判 `is_transport()` →
  直接返回 Transport，不写反馈；Parse 才占引擎预算。
- 判分：`engine/questions/judge.rs:167-184` 传输失败保持反馈槽
  `(无:首次)`，不注入错误串。
- 本票**不吸收**其新增退避层（allo 已有 provider 2 次 + completer 1 次 +
  引擎 3 轮，叠加已足够；再叠会放大最坏调用数）。

## 实施要点

### A. parse 失败窗口（纯函数，所有调用点自动受益）

- [ ] `generation/parser.rs`：保留首次 `serde_json::from_str` 的
  `line/column`（当前 `.or_else` 修复链会吞掉首个错误的位置）；`parse_candidate`
  改为返回 `(Result<T>, Option<serde_json::Error>)` 或等价结构。
- [ ] 新纯函数 `parse_failure_window(raw, slice_start, line, column) -> String`：
  绝对偏移 = `slice_start + 行首偏移 + column - 1`（serde 列号按字节，须
  退到 UTF-8 字符边界）→ 左右各 120 **字符**；窗口内含 `line/column` 标注；
  截口不 panic、不破字符。
- [ ] `reply_diagnostic` 追加窗口（保留现有长度/括号/头 120 字符）；错误串
  形如 `invalid JSON: … at line L column C; window: …`。
- [ ] 单测：中文正文中间缺逗号 → 窗口含前后中文且合法 UTF-8；围栏/散文
  包裹对象；截断 JSON（走 recover 分支时不误报窗口）；无对象时走通用兜底。

### B. 传输/parse 分流（学习侧 `AttemptFailure`）

- [ ] 新增学习侧 `generation/attempt.rs`（`pub(crate)`）：
  ```rust
  pub(crate) enum AttemptFailure {
      Transport(AppError),   // 模型没收到：Timeout/BadGateway/RateLimited/ProviderUnavailable/Conflict(非取消)
      Rejected(String),      // 收到但不可用：非法 JSON/形状/契约
  }
  ```
  附 `is_transport()`、`into_app_error()`、`from_call(Result<T, AppError>)`。
  **不新增/不修改公共错误类型**；取消仍用现有 `CANCELLED_MESSAGE` 判别。
- [ ] `structure_gen.rs`：`attempt_once` 返回 `Result<_, AttemptFailure>`；
  LLM 调用 Err → `Transport`（原样保留 AppError）；解析/契约 → `Rejected`。
  外层环：Transport 立即 `return Err(app_error)`，**不写 feedback、不进
  同指纹早停**；只有 Rejected 占轮次、回灌、算指纹。
- [ ] `mounting.rs`、`structure_ops.rs`：LLM Err → 立即上抛（不再
  `continue`/占轮）；parse/契约仍按现逻辑回灌重试。`last_was_llm` 若失去
  用途则删除。
- [ ] `service/learning_graph.rs` 生长环：`AttemptFail` 增加 transport 变体；
  起草/重裁的 `Err(AppError)` 直接 `return Err`（不写 feedback、不进
  `should_stop_early`）；评审/铸名门的 `Rejected` 行为不变。
- [ ] `service/open_judge.rs`：保留 3×90s 重试预算；**Transport 不写
  `retry_feedback`**（保持上一轮值或 None），Parse 写窗口；`thinking_degraded`
  的通道降级判定不改。
- [ ] `service/lesson.rs`：**不改重试预算与两桶分类**；parse 失败自动带
  窗口（A 的收益）。若某处把 `AppError` 文本误当契约反馈，按 B 的规则收口。
- [ ] 其余直连 `parse_json_object` 的 KB 结构/评审调用（`generation/structure.rs`）
  只享受窗口；如发现同一「传输错误进反馈」形态，按同一 `AttemptFailure` 收口，
  不另造抽象。

### C. 观测与边界

- [ ] 失败日志带 `failure_kind=transport|parse`、`attempt`、`stage`；不新增
  WS phase/事件。
- [ ] 明确不变量：Transport 永不进重试反馈槽；Rejected 才占契约轮次；
  取消仍是 `Conflict(CANCELLED_MESSAGE)` 优先。
- [ ] 写「推翻条件」：若真机日志显示传输失败被终止后任务成功率显著下降，
  再评估在 completer 层补一次性退避；不要在引擎环里恢复错误串回灌。

## 验收

- 红绿样（Scripted completer）：
  - `Timeout`/`BadGateway` → 生成调用**恰 1 次**、无 `retry_feedback`、
    返回原 `AppError`（不是「3 轮均未通过」的 `UnprocessableEntity`）。
  - 非法 JSON → 仍 3 轮；首轮反馈含 `line/column` 与 ±120 字符窗口；
    同指纹早停仍按 Rejected 工作。
  - 判分 Transport → 3 次尝试仍发生，但 `retry_feedback` 未写入错误串；
    Parse → 写入窗口。
  - 取消 → `CANCELLED_MESSAGE` 原样，不误判 Transport。
- `cargo test -p nomifun-learning` 全绿；**`nomifun-ai-agent` / `nomifun-common`
  零 diff**（用 `git diff --stat` 自证）。
- 真机：structure 一轮 + lesson 一轮，日志出现 `failure_kind` 读数；
  smoke 对应档 `EXIT=0`（真机臂串行）。

## 非目标

- 不改 `AppError` 变体、不改 ai-agent 的 `BadGateway` 盲重试、不改 provider
  4xx/额度映射（触发条件：真机日志证明 401/402 被重试，再单独立票）。
- 不新增全局退避/重试层；不改课时 3 次瞬断预算、判分 3×90s 预算。
- 不引入 learnhub 的 replay 夹具/harness。

## 风险

- 传输失败终止会降低单次任务的「自愈」次数；依赖 provider 2 次 +
  completer 1 次 + 任务重试入口兜底，需真机观察成功率。
- serde 列号是字节偏移，中文窗口换算必须退字符边界；用中文用例钉住。
- 生长环 `AttemptFail` 是评审/守卫/契约共用类型，扩变体时逐个 match 必须
  exhaustive，避免把 Rejected 误归 Transport。

## learnhub 参考阅读顺序

1. `infra/llm_retry.rs:44-153`（窗口算法与退避判定）
2. `infra/llm.rs:27-76,199,295`（kind/原文/超时转 Transport）
3. `engine/questions/judge.rs:167-184`（判分反馈槽）
4. `engine/contentgen/facade.rs:648,778`（引擎环分流）
