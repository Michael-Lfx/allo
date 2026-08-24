# Session Observation 开发文档与分步实施计划

> **最后维护：** 2026-08-24（元数据复核，未重写结论；核对基准 commit `d791691c6`）  
> **现行 UI 以 [session-observation-workflow-ui-plan.zh.md](session-observation-workflow-ui-plan.zh.md) 与 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准，文内 Drawer 已作废。**  
> **文档状态：S0–S8 历史步骤；U0–U5 与 PR #119 收口（配额 GC、请求扫描列表）已合入 `main`。不要把文内 S 步骤当成待办。**  
> 日期：2026-08-20  
> **Baseline：** `6c50755db`（`main`，Merge PR #119）  
> **现行语义：** [session-observation-workflow-ui-plan.zh.md](session-observation-workflow-ui-plan.zh.md)（配额 GC、无按天 TTL、请求 messages/tools 扫描列表）  
> 契约原文：[session-observation-workflow-proposal.zh.md](session-observation-workflow-proposal.zh.md)  
> 现状说明：[agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md)

本文保留 S0–S8 怎么走完 JSONL 数据路径，**不要**再当未授权的实施入口。现行语义看 workflow-ui-plan 与 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md)。

---

## 1. 产品拍板（相对提案的增量）

| 项 | 决定 |
| --- | --- |
| 旧 Trace | **直接替换并删除**。项目未正式发布，不保留 collector、旧 API、投影兼容层、回退开关。 |
| 开发过程 | crate 内允许短暂并存，便于分步合入；**最后一步必须删干净**，不得把双轨带进可合并结果。 |
| 端到端 | 最终必须能：记录 → 落盘 → 查询 → Developer Drawer 看见请求/响应/工具。不允许只改记录层却留空 UI，也不允许只改 UI。 |
| 插件化 | **不**先做 `ObservationSink[]`、用户配置、OTel、导出。一个 JSONL writer 即可。 |
| Context | **显式传递**（`stream_llm(..., ctx)` / `ObservationSession`）。禁止 async thread-local。 |
| 身份 | 映射已有 ID。禁止无限定词 `Run`。Provider 重试字段禁止叫 `attempt_id`。 |
| 默认 capture | persist 前：`canonical + truncated + redacted`；媒体 `metadata_only`。 |

---

## 2. 目标与非目标

### 2.1 本轮要交付

开发者在 Developer Drawer 里按用户回合看到：

1. canonical `LlmRequest`（system / messages[] / tools / 采样），按 capture policy；
2. 模型响应（text / thinking / tool_use 意图、usage、TTFT、耗时）；
3. 工具执行生命周期（started / completed / failed / cancelled）；
4. `integrity=degraded`：`observation/gap` 或缺少 terminal 的 `interrupted`；
5. 观测失败不打断 Agent 回合。

### 2.2 本轮明确不做

- HTTP wire body、OTel、导出 UI、会话一等 Workflow Tab、replay / 业务恢复；
- ACP / 外部 CLI 完整请求信封（契约预留 `protocol_partial` 即可）；
- 用户可配 sinks / retention UI；
- 把观测 log 做成产品对话 SSOT；
- 合并 Eval（`nomi-agent-eval`）。

---

## 3. 推荐落点（最小，不改 `LlmProvider`）

```text
Caller
  显式 ModelCallContext / ObservationSession.bind_ids
        |
        v
stream_llm(provider, request, observer, call_kind, scope)
        |
        +--> LlmProvider::stream   （raw，不改 nomi-providers）
        |
        +--> ObservationRecorder.emit   （已 apply capture）
                |
                v
        {data_dir}/diagnostics/observation/{conversation_id}/events.jsonl
                |
                v
        query / project_turns
                |
                v
        GET /api/debug/session-observations
                |
                v
        Developer Drawer（替换今日 AgentTraceInspector）
```

| 组件 | 放哪 | 说明 |
| --- | --- | --- |
| 事件 / capture / JSONL / 投影 | `nomi-agent-trace` | 升级本 crate，不新开 crate。`nomi-providers` 不得依赖它。 |
| `stream_llm` + `ObservationSession` | `nomi-agent` | 显式包装，不改 `LlmProvider` trait。 |
| 开发者模式门控 + HTTP | `nomifun-ai-agent` + `nomifun-conversation` | 替换 `AgentTraceHub` / `TurnTraceCollector` / `/api/debug/agent-traces`。 |
| UI | 现有 Inspector 挂载点 | 换数据和呈现，不先做一等 Tab。 |

**Recorder 共享：** 用 `data_dir` intern 同一个 `ObservationRecorder`（factory 与 Hub 各拿一份 Arc）。不要为了接线去加长 `NomiAgentManager::new` 的参数列表。这是进程内存储 intern，不是隐式业务 context。

**执行边界（钉死）：** `event_seq` 归属优先 `root_turn_id`，否则 `execution_id`，否则 `conversation_id`，否则 `process`。子执行用自己的 seq。查询排序只看 `event_seq`。

**落盘常量（钉死，M4 再做成配置）：**

| 常量 | 值 |
| --- | --- |
| 根目录 | `{data_dir}/diagnostics/observation/` |
| 分文件 | 按 `conversation_id`；无会话走 `process/` |
| rotate | 单个 `events.jsonl` ≥ **48 MiB** → `events.{n}.jsonl` |
| flush | DualQueue writer：batch / ~75ms / Shutdown；热路径不每条 fsync。读路径 `flush_for_read`（约 500ms）后读盘 |
| retention | 高低水位（高 **1 GiB** / 低 **800 MiB** / 紧急 **1.2 GiB**）。写盘空闲 ≥30s 才扫盘（紧急超标除外）；首次队列空档只 reconcile 估算（含盘上存量）。额度一次收到低水位，不削到刚好 1 GiB |

**采集门控：** 采集始终开启（会话发送即写盘）。开发者模式只门控 HTTP 读取与支持包附带 JSONL；API 仍须服务端校验 developer mode。

---

## 4. 现状与调用点清单

实施每个阶段前必须再 `grep` 一遍，路径会漂移。

### 4.1 今日旧 Trace（将被删除）

| 层 | 路径 |
| --- | --- |
| 模型 / 存储 | `crates/agent/nomi-agent-trace/`（`TurnTrace`、`FileTraceStore`、`TurnTraceBuilder`） |
| 采集 | `crates/backend/nomifun-ai-agent/src/agent_trace/collector.rs` |
| Hub | `.../agent_trace/hub.rs`，装配于 `nomifun-app/src/router/state.rs` |
| 触发 | `nomifun-conversation/src/service.rs` 在 `send` 前 `TurnTraceCollector::spawn` |
| API | `nomifun-conversation/src/routes_trace.rs` → `/api/debug/agent-traces` |
| UI | `ui/src/renderer/pages/conversation/components/AgentTraceInspector/` |
| 支持包 | `nomifun-system/src/support_logs.rs` 的 `diagnostics/observation` |
| Eval 对齐 | `nomi-agent-eval` 只引用 `SCHEMA_VERSION` |

### 4.2 Nomi-owned `.stream(`（必须分类接入）

| 位置 | 建议 `observation_scope` | 建议 `call_kind` | 阶段 |
| --- | --- | --- | --- |
| `nomi-agent/src/engine/mod.rs` 主 loop | `session_workflow` | `agent_turn` | S3 |
| `nomi-agent/src/compact/auto.rs` | `session_workflow` | `compaction` | S4 |
| `nomi-agent/src/goal/judge.rs` | `session_workflow` | `goal_judge` | S4 |
| `nomi-agent/src/moa/runner.rs` `collect_advice` | `session_workflow` | `moa` | S4 |
| `bootstrap.rs` `SessionExtractModel` | `session_workflow` | `browser_extract` | S4 |
| `bootstrap.rs` `SessionVisualLocator` | `session_workflow` | `visual_locate` | S4 |
| `nomifun-ai-agent/src/one_shot.rs` | 有会话则按规则；无会话 `process_diagnostic` | 调用方短名 | 本轮可跳过 |
| `factory/provider_config.rs` 探测 | `process_diagnostic` | — | 本轮不采集 |
| `nomifun-robot/.../speech.rs` | `process_diagnostic` | — | 本轮不采集 |
| `nomi-providers` 测试 | — | — | 不改 |

补充规则（不枚举功能名）：影响用户可见回合 → `session_workflow`；只改会话元数据 → `session_auxiliary`；没有会话 → `process_diagnostic`。

注意：`MoaRunner::run` 目前主要被自身测试调用，engine loop 是否真正 fan-out 必须在 S1 inventory 复核。若生产路径未调用，S4 只给 `collect_advice` 留显式 observer 参数，不虚构接入。

### 4.3 工具执行

`nomi-agent/src/tool_execution.rs` 的 `execute_tool_calls_scoped` / `execute_tool_calls_with_approval`。  
S5 优先在 engine 调用前后旁路 emit（started → 执行 → completed/failed/cancelled），避免先改工具调度签名。模型意图已在 `llm/response.tool_use`。

---

## 5. 分步计划（一次只做一步）

每步结束必须：相关测试通过；不扩大到下一步的文件。未授权下一步时停住。

### S0 — 文档冻结（本步）

**做：** 提案 + 本文；记录 baseline；实现原则入库。  
**不做：** 任何业务代码。  
**验收：** Reviewer 能按本文逐步开工，不再回头改架构方向。

### S1 — 契约类型与 fixture（只动 `nomi-agent-trace` 增量）

**做：**

- 新增 `ObservationEvent` 信封（`schema_version` / `event_type` / `event_seq` / `timestamp` / `payload`）。
- 新增 `ObservationIds` / `ModelCallContext` / `observation_scope` / `call_kind` / `fidelity` / `capture` / `integrity`。
- Reader：未知 `event_type` 跳过或保留 raw。
- 先**不要删** `TurnTrace` / `FileTraceStore`（下一步还靠它们编译）。

**验收：** `cargo test -p nomi-agent-trace`  
覆盖：JSON 往返、未知 event、boundary 优先级（turn > execution > conversation > process）。

### S2 — Capture + JSONL + 投影（仍只动 `nomi-agent-trace`）

**做：**

- persist 前 capture：redact + truncate；Image/base64 → metadata（hash / mime / byte_length / `omitted_reason=binary_payload`）。
- `ObservationRecorder`：按会话分文件、48 MiB rotate、flush、1 GiB/800 MiB 高低水位（闲时扫盘，紧急 1.2 GiB；首次空档 reconcile 估算）。
- `project_turns`：按 `event_seq` 排序；缺 terminal → `interrupted` + `integrity=degraded`；`observation/gap` 降级该执行边界。

**验收：** 媒体 fixture 不含 base64 实体；interrupted / gap 测试；rotate 单测。  
**不做：** engine、API、UI。

### S3 — 主 loop 采集

**做：**

- `nomi-agent` 增加 `ObservationSession` + `stream_llm`。
- `AgentEngine` 增加 optional `observation`；`new_with_provider` / 测试字面量补 `None`。
- 仅替换主 loop 的 `provider.stream`。
- `NomiAgentManager` 用 `ObservationRecorder::shared(data_dir)` 创建 session；`send_message` 前 `bind_ids`（`conversation_id` / `msg_id` / `root_turn_id` / `session_kind`）。
- Hub 只门控 HTTP 读 API，不调用 `recorder.set_enabled`（采集始终开启；按开发者模式开关写盘已作废）。

**验收：** `cargo test -p nomi-agent`；一条带 observer 的单测能从 JSONL 读到 `llm/request` + `llm/response`。  
**不做：** compact/judge/工具/UI。旧 collector 此步仍可暂时存在。

### S4 — 其余 Nomi-owned 调用

**做：** compact、goal judge、（若生产确有调用）MoA、browser extract、visual locate。全部显式传 session / context。  
**验收：** 各调用点不再直接 `provider.stream`（inventory 复检）；至少 compact + judge 有观测单测或契约测试。  
**不做：** one_shot / 探测 / 语音。

### S5 — 工具生命周期

**做：** 在 engine 执行工具前后 emit `tool/execution_*`；`Quit` → cancelled。参数/结果走已有 `redact_json_value`。  
**验收：** Agent→tool→Agent 样本里能按 `model_call_id` + `tool_call_id` 对齐意图与执行。

### S6 — 替换读 API，删除 collector

**做：**

- `GET /api/debug/session-observations?conversation_id=`
- `GET /api/debug/session-observations/turns/{root_turn_id}?conversation_id=`
- 删除 `TurnTraceCollector::spawn` 及 collector 模块。
- 开发者模式门控迁到新 Hub 方法。

**验收：** `cargo test -p nomifun-ai-agent`；旧 `/api/debug/agent-traces` 不再注册。  
**不做：** 完整新 UI（可用临时 JSON 验证）。

### S7 — 替换 Developer Drawer

**做：** 复用 `ChatLayout` 挂载点，重写 Inspector：左回合、右 REQUEST→RESPONSE→tools、integrity / gap / interrupted 可见。禁止用聊天气泡拼 `messages[]`。i18n：`zh-CN` + `en-US`。  
**验收：** `bun run typecheck`；`bun run check:i18n`；结构测试改指向新 API。

### S8 — 删除旧 Trace 并收口文档

**做：** 删除 `TurnTrace` / `TurnTraceBuilder` / `FileTraceStore` / 旧 hub 类型导出 / 旧 UI 类型。`support_logs` 与 lifecycle ZIP 改为 `diagnostics/observation/`。更新 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md)。提案阶段表留 Git 历史，不形成第三套长期真相。  
**验收：**

```text
cargo test -p nomi-agent-trace
cargo test -p nomi-agent
cargo test -p nomifun-ai-agent
cargo check -p nomifun-conversation
bun run typecheck
bun run check:i18n
```

全库 `rg TurnTrace|agent-traces|/api/debug/agent-traces|TurnTraceCollector` 应为空（文档历史引用除外）。

### 延后（不是本轮阶段）

S9+：用户 sinks 配置、导出、OTel、ACP 采集、wire body、会话一等 Tab、replay。

---

## 6. 关键实现约束（写代码时对照）

1. 先读调用链再改，不凭局部猜测。
2. `LlmRequest` 不是 HTTP body；只记 canonical。
3. Capture 发生在 `emit` 内、写盘前。Sink 不得先收 raw。
4. `stream_llm` / 工具 emit 失败只 `warn`，必要时写 gap；不 `?` 打断回合。
5. 查询层禁止用 `timestamp` 排序重建工作流。
6. 禁止无限定 `Run`；AgentExecution 的 attempt 用 `execution_attempt_id`。
7. HTTP DTO 可先跟现有 traces 一样直接序列化 agent 层投影类型；不要为此新开一层无调用方的 api-types。
8. 发现无关问题只记录，不顺手改。

---

## 7. 工作区纪律

S1–S8 已落地。运行时契约以 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准；本文保留阶段史，不形成第三套长期真相。

2026-08-18 曾有一次未完成的 crate 改写（删了 `TurnTrace` 但未接完 API），随后回退并以本文件为实施依据重新落地。

之后每个阶段（历史约束，供读阶段表时对照）：

1. 只改该阶段列出的文件；
2. 跑该阶段验收命令；
3. 停下来等授权下一步。

---

## 8. 授权

- 本文 + 提案 = 开发依据。
- **现在不授权改实现代码。**
- 下一步若授权，从 **S1** 开始。
