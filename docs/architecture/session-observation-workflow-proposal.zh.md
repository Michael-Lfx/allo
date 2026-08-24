# Session Logs / 工作流观测：背景、对照与分阶段方案

> **最后维护：** 2026-08-24（元数据复核，未重写结论；核对基准 commit `d791691c6`）  
> **现行 UI 以 [session-observation-workflow-ui-plan.zh.md](session-observation-workflow-ui-plan.zh.md) 与 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md) 为准，文内 Drawer 已作废。**  
> **文档状态：契约考古；S1–S8 / U0–U5 已落地，配额 GC 与请求扫描列表随 PR #119 合入 `main`。不要把文内 S 步骤当成待办。**  
> 撰写日期：2026-08-18  
> 修订：同日吸收多轮 review；并确认**直接替换旧 Trace**（项目未发布，无双轨）  
> **Baseline commit：** `fcf5c4e203da2b30a05444729ff8108aa9bd22d9`（`main`，Merge PR #106）  
> **怎么做（历史）：** [session-observation-implementation.zh.md](session-observation-implementation.zh.md)  
> 读者：契约与决策说明。现行产品语义以源码 + [Agent 可观测性与评测](agent-observability-and-eval.zh.md) 为准。

本文回答四件事：

1. 为什么要做 Session Logs / 工作流观测；
2. DeepSeek Harness 与 `dsh-plugin-agent-workflow` 的优势，以及对 Flowy 的影响；
3. 已拍板的目标、非目标与 **M0 冻结契约**；
4. 基于 nomi 架构、可分阶段交付的实现过程。

相关已落地文档（描述现状，不是本提案）：

- [Agent 可观测性与评测](agent-observability-and-eval.zh.md)
- [Agent 引擎](agent-engine.zh.md)
- [Agent Execution](agent-execution.zh.md)
- [Agent Harness 能力审查](agent-harness-capability-analysis.zh.md)
- [Agent Loop 边界](agent-loop-comparison.zh.md)

对照源码（仓库外，仅作参考，不是依赖）：

- DeepSeek Harness：<https://github.com/deepseek-ai/DeepSeek-Harness>
- Agent 工作流插件：<https://github.com/xuanyuanzhifeng/dsh-plugin-agent-workflow>

---

## 给 Reviewer 的结论

1. **要解决的问题**：开发者需要看到一次对话里「交给 `LlmProvider` 的 canonical 请求、模型返回、工具调用细节与时间轴」，用来排查和优化。现有 Developer Mode Trace 只能提供截断后的 span 预览，不能重建请求信封。
2. **不要做什么**：不要把 nomi 底层改造成 Cordis；不要把 `dsh-plugin-agent-workflow` 装进 Flowy；不要把该插件的 TypeScript 类型当成存储 / API 契约；不要新造无限定词的 `Run` 领域。
3. **已拍板（方案 B）**：对齐该插件的**信息与呈现**，Flowy 使用**自有观测 schema**。插件字段是清单，不是 wire 同构目标。
4. **能做的范围**：M1 只承诺 **canonical `LlmRequest`**（`fidelity=canonical`），不是 HTTP body。ACP / 外部 CLI 为 `protocol_partial`。
5. **落地顺序（历史）**：按 [实施文档](session-observation-implementation.zh.md) 的 S1–S8 已逐步做完。旧 Trace **删除替换**，不投影兼容。本文保留契约，不再授权新的实现改动。

---

## 1. 背景

### 1.1 产品与架构约束

Flowy 是 Rust + Tauri / Web 双宿主的本地优先 AI 工作站。Agent 引擎在 `crates/agent`（`nomi-*`），业务后端在 `crates/backend`（`nomifun-*`），宿主经 `nomifun-ai-agent` 桥接。单轮循环在 `nomi-agent::AgentEngine`；跨会话协作执行在 `nomifun-agent-execution`，聚合类型只有 **AgentExecution**（见 [agent-execution.zh.md](agent-execution.zh.md)）。观测方案不得把业务执行状态再塞回单轮 engine，也不得发明与 AgentExecution 并列的 `Run` 实体。

当前对话真相源是 **SQLite 产品消息表**，加上 WebSocket 上流式的 `AgentStreamEvent`。这套数据服务于用户可见聊天，不保证等于「某一次 `LlmProvider::stream` 收到的 canonical `LlmRequest`」，更不等于 HTTP body。

### 1.2 开发者真正缺什么

排查与优化需要同时看到：

| 需要 | 典型问题 |
| --- | --- |
| 交给 Provider 抽象的信息 | system 实际拼了什么；messages[] 是否含 skill / memory / turn tail；当次 advertise 了哪些 tools |
| 模型返回 | reasoning / content / tool_use、stop_reason、usage、TTFT |
| 工具细节 | 模型意图 vs 是否真正执行；参数、结果、错误、耗时、产物 |
| 组织方式 | 按用户轮次 → 该轮内第 N 次模型调用 → REQUEST → RESPONSE → tools |

现有聊天气泡、SQLite 历史、context 估计**不能**可靠反推上述请求体。用产品消息「拼一个看起来像的 messages[]」会误导排查，明确禁止。

### 1.3 Flowy 已有观测（以及缺口）

已落地能力见 [agent-observability-and-eval.zh.md](agent-observability-and-eval.zh.md)。摘要：

| 层 | 现状 | 证据 |
| --- | --- | --- |
| 存储 | `TurnTrace` + `TraceSpan`，目录 `{data_dir}/diagnostics/agent-traces/` | `crates/agent/nomi-agent-trace` |
| 采集 | `TurnTraceCollector` 订阅 `AgentStreamEvent`，开发者模式才写盘 | `crates/backend/nomifun-ai-agent/src/agent_trace/` |
| API | `/api/debug/agent-traces`（list / recent / artifacts / `{trace_id}`） | `crates/backend/nomifun-conversation/src/routes_trace.rs` |
| UI | 会话页 `AgentTraceInspector` + `TraceTimeline` | `ui/src/renderer/pages/conversation/components/AgentTraceInspector/` |
| 门控 | `system.developerMode`；密码只是前端 UX 门槛 | `ui/src/common/config/developerMode.ts` |

`LlmRequest` 在 engine 内存中已经完整存在，并立刻交给 `provider.stream`：

```1593:1606:crates/agent/nomi-agent/src/engine/mod.rs
            let request = LlmRequest {
                model: self.model.clone(),
                system,
                messages,
                tools,
                max_tokens: self.max_tokens,
                thinking: self.thinking.clone(),
                reasoning_effort: self.current_reasoning_effort.clone(),
                temperature: None,
            };

            efficiency.observe_model_turn_attempt();
            let stream_start = std::time::Instant::now();
            let mut rx = self.provider.stream(&request).await?;
```

这是 **canonical request**，不是 HTTP body。`nomi-providers` 的 OpenAI / Anthropic adapter 还会再序列化（system 折进 messages、tool schema 清洗、`stream_options` 协商等）。部分 retry 会改 body，部分只换 key / 复用 body。

`AgentStreamEvent::RequestTrace` 存在，前端注释写明 **not persisted**。Collector 走 broadcast，可能 `Lagged`。因此现有槽位**不是**新观测 SSOT。

除主 loop 外，至少还有独立 `LlmRequest`：`compact/auto.rs`、`goal/judge.rs`、`moa/runner.rs`、`bootstrap.rs`（extract / visual locate，后者 messages 可含 base64 PNG）、以及宿主侧 `one_shot` / 模型探测 / 语音等。只打 `AgentEngine` 会破坏「Nomi-owned 调用可观测」不变式。

---

## 2. 其他项目的优势与影响

### 2.1 DeepSeek Harness（dsh）

公开口径（developer preview，2026-08）：Everything is a plugin；Every run is traceable。

| 优势 | 含义 | 对 Flowy 的影响 |
| --- | --- | --- |
| Cordis 插件内核 | 模型、工具、session、loop、UI 均可配置替换 | **不要移植。** |
| Append-only session log | 观测 SSOT；resume / fork / replay 投影自此 | **借鉴理念，不替换 SQLite 产品消息表。** |
| Model-visible means recorded | 进入模型的内容必须可从 log 重建 | **限定为 Nomi-owned invocation + 显式 gap/interrupted。** |
| Trajectory / Persistence 可换 | 视图与存储都是投影 / Sink | Sink/View 可组装；不跟 dsh RC 同构。 |

dsh 仍在 developer preview，兼容性破坏是预期。

### 2.2 `dsh-plugin-agent-workflow`

独立 Web UI 插件（MIT），钉死 `dsh@0.1.0-rc.7`。比原生 Trajectory 更适合排查：按用户轮次、横向 REQUEST→RESPONSE→tools、真实落盘的 system / messages[] / tools JSON。**插件只消费事件，不采集。** 没有信封，UI 只能「长得像工作流」。

### 2.3 正确吸收（方案 B）

借：信息清单、交互骨架、可记录不变式、Sink/View 可组装。  
不借：Cordis、该插件包、dsh 类型作存储 SSOT、resume/fork/replay 业务化（首期不做）。

---

## 3. 已拍板决策

| 决策 | 选择 |
| --- | --- |
| 对齐方式 | **B：信息与界面等价，自有 schema** |
| 运行时 | 不引入 Cordis |
| 请求分层 | M1 只记 **canonical `LlmRequest`**；wire HTTP body 延后 |
| 插件化 | 本轮一个 JSONL writer；用户配置 / OTel / 导出延后 |
| 产品消息表 | SQLite 仍是用户对话 SSOT；Observation Log 是**新诊断 SSOT** |
| 身份 | 映射已有 ID，不新造 `Run` 领域；不用 `attempt_id` 表示 AgentExecution Attempt |
| 采集落点 | `ObservedProvider` 包 `dyn LlmProvider`；Caller 显式传 `ModelCallContext` |
| 覆盖 | 所有经 ObservedProvider 的 Nomi-owned 调用可记录；Workflow 按 `observation_scope` 过滤 |
| 丢失 | 禁止静默丢；`observation/gap` 或投影 `interrupted` |
| 默认 capture | `canonical + truncated + redacted`；媒体 `metadata_only` |
| 脱敏时机 | **持久化之前**，禁止先写 raw 再靠 API 脱敏 |
| 入口 | 开发者模式；M3 先扩 Developer Drawer，不是会话一等 Tab |
| UI 时机 | 先记录后界面 |
| 子执行 | 会话是容器；协作树用 `execution_id` / `step_id` / AgentExecution `attempt_id` |
| 分类规则 | 影响用户可见回合 → `session_workflow`；只改元数据 → `session_auxiliary`；无会话 → `process_diagnostic` |
| Context 传递 | **显式** `stream_with_context` / observed call API；禁止 async thread-local 作核心机制 |

开发者模式解锁口令是前端临时 UX 门槛，**不是安全边界**。观测 API 仍须服务端校验 developer mode。`full` capture 须显式开启 + retention + redact + 导出确认。

---

## 4. 实现目标

### 4.1 目标

与 `dsh-plugin-agent-workflow` **同等信息密度**的排查（自有字段名）：

1. 按用户轮次看到该轮模型调用；
2. 请求详情：canonical `system` / `messages[]` / `tools` / 模型与采样（按 capture policy）；
3. 响应：reasoning / content / tool_use、usage、stop、耗时 / TTFT；
4. 工具：模型意图与执行生命周期分开；
5. 会话 / 执行边界汇总与 integrity；
6. 观测失败不打断回合；静默丢失必须变成显式 gap 或 interrupted。

### 4.2 非目标（M1 锁定不做）

- wire HTTP body、OTel、导出 UI、会话一等 Workflow Tab、replay / 业务恢复；
- 把观测 log 做成产品对话持久化；
- 保证 ACP 具备完整请求 JSON；
- 把 Eval 与观测 log 合并。

### 4.3 完整度承诺

| 运行时 | M1 | fidelity |
| --- | --- | --- |
| nomi 经 ObservedProvider | 必须采集 | `canonical` |
| ACP / 外部 CLI | M1 可不采集；契约预留 | `protocol_partial` |
| 渠道 / companion / cron | 有 `conversation_id` 则写入该会话；默认 Workflow 仍以 `session_dialogue` 为主 | 视路径，禁止假装 canonical full |

---

## 5. 呈现清单（插件字段 → Flowy 事件）

| 工作流需要呈现 | Flowy 事件 / 字段 | 来源 |
| --- | --- | --- |
| 用户轮次 + 提示摘要 | `turn/start`（`conversation_id`、`msg_id` / `root_turn_id`） | 用户消息 + engine |
| REQUEST | `llm/request`（canonical） | ObservedProvider，stream 前 |
| RESPONSE | `llm/response`（含 `tool_use` 意图） | stream 结算 |
| 工具执行 | `tool/execution_started` / `completed` / `failed` / `cancelled` | 工具执行层 |
| Token / 缓存 | `llm/response.usage` | provider `Done` |
| 汇总 | 投影自事件索引 | query |
| 省略 | `omitted[]` | capture policy |
| 已知丢事件 | `observation/gap` | Bus / Sink |
| 无终态 | 投影 `interrupted` | Reader |

---

## 6. 建议架构

```text
Caller (engine / compact / judge / moa / bootstrap …)
    ModelCallContext（显式）
            |
            v
    ObservedProvider          ← nomi-agent 或宿主装配，包 dyn LlmProvider
            |
            v
    LlmProvider（raw，adapter 内才有 wire）
            |
            v
    ObservationSink[]         ← 只接收已 apply capture policy 的事件
       └── JsonlSink（本轮只有这一个 writer）
```

- `ObservedProvider` **不知道**业务语义；`call_kind` / `observation_scope` 由 Caller 放入 `ModelCallContext`。
- Session runtime **不应轻易拿到 raw provider**。`process_diagnostic`（探测、独立语音）才允许裸 provider。
- `grep .stream(` 只作 M0/M1 inventory，不是长期门禁。
- `nomi-providers` 不得依赖 `nomi-agent-trace`。

### 6.1 身份映射（禁止新造 Run 领域）

观测内部可用短名，但对外与契约必须映射现有 ID：

| 观测概念 | 映射到 | 禁止 |
| --- | --- | --- |
| 会话容器 | `conversation_id` | 另造 session 产品实体 |
| 用户回合 | `msg_id`、`root_turn_id` | — |
| 协作 / 子 Agent | `execution_id`、`step_id`、AgentExecution 的 `attempt_id` | 无限定词 `Run` 作为领域类型 |
| 一次模型调用 | `model_call_id` | — |
| 事件顺序 | 执行边界内 `event_seq` | 用 timestamp 排序重建工作流 |
| Provider HTTP 重试（未来） | 预留字段，**不要叫 `attempt_id`**（与 AgentExecution Attempt 重名） | M1 不产生 wire attempt 事件 |

父子关系：`parent_execution_id` / `parent_model_call_id`（若需要）。Workflow 默认显示主回合，Child Execution 先给入口再展开。

已有 `session_kind`（`session_dialogue` / `companion` / `cron`…）继续描述**产品表面**。`observation_scope` 只决定记不记 / 默认展不展。`call_kind` 只描述这次模型用途。

### 6.2 调用分类规则（不枚举功能名）

| 规则 | `observation_scope` | 默认进 Workflow UI |
| --- | --- | --- |
| 影响当前用户可见回合（含 compaction、judge、moa、browser extract、visual locate） | `session_workflow` | 是 |
| 只改会话元数据（如标题） | `session_auxiliary` | 默认隐藏 |
| 没有会话（探测、独立语音等） | `process_diagnostic` | 否 |

新增模型用途套规则，不必每次改架构文档。

---

## 7. M0 冻结契约（Vocabulary v1）

信封形状：

```text
schema_version = 1
event_type
event_seq          // 见 7.1
timestamp          // 仅耗时与展示
payload
```

### 7.1 排序（M0 blocker）

- `event_seq` 在一个**观测执行边界**内单调递增（建议边界 = 一次用户回合 `root_turn_id`，或一次 AgentExecution `execution_id`，二者取实际写入时的归属；子执行用自己的 seq，不与父混编）。
- `model_call_id` 关联同一次调用的 `llm/request` 与 `llm/response`（及该调用下的 tool 执行）。
- **查询 / 投影排序优先 `event_seq`，禁止用 timestamp 重建顺序。** timestamp 只用于耗时与展示。
- 并发（async tool、streaming、child execution）下 timestamp 相同或乱序是预期。

### 7.2 分类与完整度（拆维）

```text
observation_scope: session_workflow | session_auxiliary | process_diagnostic
call_kind:         由 Caller 提供的稳定短名（agent_turn / compaction / …）

fidelity:  canonical | wire | protocol_partial | unknown
capture:   full | redacted | truncated | metadata_only
integrity: complete | degraded     // 按执行边界，不按整个 conversation
```

M1 默认：`fidelity=canonical`，`capture=truncated` **且** 必做 redact，媒体 `metadata_only`。  
ACP 契约：`fidelity=protocol_partial`，禁止拼假 `messages[]`。  
`wire` 为 future，M1 不采集。

`omitted[]` 说明截断 / 脱敏 / 媒体省略原因。禁止 UI 补全 omitted 字段。

### 7.3 模型调用与工具语义

```text
llm/request
llm/response            // 含 tool_use 意图（模型想调什么）
tool/execution_started
tool/execution_completed
tool/execution_failed
tool/execution_cancelled
```

模型想调但 validation 未执行 ≠ 工具执行失败。用 `model_call_id` + `tool_call_id` 关联。

### 7.4 丢失与崩溃（M0 blocker）

**已知丢（队列满、Sink 失败等）：** 写 `observation/gap`（reason、from_seq、to_seq 或 lost count）。执行边界 `integrity=degraded`。UI 顶栏标 degraded，时间轴画 gap：「不是没事发生，而是观测缺失」。

**进程崩溃 / 无终态：** `llm/request` 之后没有对应 terminal（`llm/response` 或明确错误终态）时，**读取/投影**将该 `model_call_id` 标为 `interrupted` / `unknown`，对应执行边界 `integrity=degraded`。这不是 queue gap，不必伪造 `observation/gap` 行，但不得显示为完整。

不变式改为：

> 所有经 ObservedProvider 的 Nomi-owned 调用必须有观测记录，或存在显式 gap / interrupted；不得静默丢失后仍声称该执行边界完整。观测不得阻塞 Agent 回合。

### 7.5 媒体与脱敏（M0 blocker）

canonical `LlmRequest.messages` **已经可能含 Image（base64 PNG）**（visual locate）。不得假设 canonical = 安全小 JSON。

默认只记：`type` / `mime` / `byte_length` / `hash` / `source_ref` / `omitted_reason=binary_payload`。禁止默认把 binary/media 内联进 JSONL。

**任何持久化 Sink 默认只接收已经执行 capture policy 的事件；不得先把原始 secret / full payload / 媒体字节写盘，再由 API 或 UI 脱敏。**

### 7.6 Schema 演进

- Reader 遇到未知 `event_type`：**跳过或保留 raw**，不得整段 Session 无法读取。
- 字段新增默认向后兼容。
- 只有破坏语义才 bump `schema_version` major。
- 未知 required 语义（未来若有）的处理单独说明；v1 无此类字段。

### 7.7 M1 最小落盘规则（写清即可，不扩大 M1）

完整用户配置留 M4。M1 至少规定：

| 项 | M1 规则 |
| --- | --- |
| 根目录 | `{data_dir}/diagnostics/observation/`（旧 `agent-traces/` 在 S8 删除） |
| 分文件 | 按 `conversation_id`（无会话的 diagnostic 用独立 `process/` 前缀） |
| 单文件阈值 | 超过实现常量（建议约 32–64 MiB）则 rotate 到带序号的新文件；索引指向 active |
| shutdown | graceful shutdown **必须 flush** 已接受事件 |
| retention | M1 可用固定天数或文件数上限做最简 GC；可配置化留 M4 |

---

## 8. 分阶段实现过程

### M0 — 契约与 fixture（不写大 UI，不改热路径）

- 把第 7 节落成类型 + 契约测试 / fixture（自有 JSON）。
- 覆盖：canonical truncated+redacted、媒体 metadata_only、ACP protocol_partial、gap、interrupted、未知 event_type。
- inventory：列出当前 `.stream(` 调用点并标 scope（可一次性静态搜索）。

**验收：** 往返序列化；未知 event 不炸 reader；媒体 fixture 不含 base64 实体。

### M1 — Canonical Observation MVP

必须：主 loop、compaction、goal judge、MoA、browser extract、visual locate、工具执行生命周期、gap、JSONL、capture 在持久化前应用。  
不做：wire、OTel、导出、一等 Tab、replay。

采集：Caller 显式 `ModelCallContext` → `ObservedProvider`。失败只 warn，并尽量写 gap。

**验收样本（禁止只测「聊一句」）：** Agent→tool→Agent；compaction；goal judge；MoA（若启用）；vision/extract；provider 协商类失败（不要求记 wire）；人为制造队列满或崩溃后的 gap / interrupted 投影。

### M2 — 删除旧 Trace（不再投影兼容）

项目未正式发布：**直接删除** `TurnTraceCollector`、旧 debug API、旧 Inspector 数据模型。开发过程允许 crate 内短暂并存，可合并结果不得保留双轨。详见实施文档 S6–S8。

### M3 — 工作流 UI

扩 Developer Inspector / Drawer：`Trace` / `Workflow` / `Raw Events`。信息密度对齐插件截图。`protocol_partial` 与 `interrupted` / gap 必须可见。不先做会话一等 Tab。

### M4 — 配置化 Sink、导出、retention UI、可选 ACP 采集、可选 wire

---

## 9. 对当前架构的影响与风险

| 区域 | 影响 | 控制 |
| --- | --- | --- |
| `nomi-agent` | ObservedProvider + 各 Caller 传 context | 显式 API；不改 `LlmProvider` crate |
| `nomi-agent-trace` | event log + policy + reader | schema_version；未知事件可跳过 |
| 磁盘 / 隐私 | 仍可能大 | persist 前 redact；媒体 metadata；rotate |
| AgentExecution 词汇 | 观测树映射 execution/step | 禁止新 Run 领域 |
| 产品消息 | 不变 | 观测不得反向驱动业务 |
| 旧 Trace | 分步替换后删除 | 单一诊断 SSOT，无兼容层 |

质量门：`cargo test -p nomi-agent-trace` / `nomi-agent` / `nomifun-ai-agent`；前端 `bun run typecheck`；i18n `bun run check:i18n`；勿触发 `check-agent-vocabulary`（禁止无限定 `Run`）。

---

## 10. Review 问题闭环

原第 9 节六个问题已拍板，不再作为开放问题：

1. 不变式限定 Nomi-owned + gap/interrupted；ACP = protocol_partial。  
2. 落点 = ObservedProvider + 显式 context，不是只打 engine、也不是只打宿主桥。  
3. 旧 Trace 删除替换，不投影兼容。  
4. 默认 truncated+redacted 够用，须保留结构 / 计数 / hash / omitted。  
5. 本轮只要一个 JSONL writer；用户 sinks 配置延后。  
6. UI 先 Drawer，不一等 Tab。

---

## 11. 落地状态

S1–S8 与 U0–U5 已落地；配额 GC（无按天 TTL）与请求扫描列表随 PR #119 合入 `main`。现行语义以 [ui-plan](session-observation-workflow-ui-plan.zh.md) 与 [Agent 可观测性与评测](agent-observability-and-eval.zh.md) 为准。本文保留契约考古，不再当作待办。

已钉死的实现常量见实施文档 §3：rotate 48 MiB、retention 1 GiB 高低水位、`event_seq` 归属 turn > execution > conversation > process。
