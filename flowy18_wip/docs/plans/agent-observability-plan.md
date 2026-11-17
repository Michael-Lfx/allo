# Agent 可观测性优化 Plan

状态：待确认（不写代码，仅方案）。目标是用最小改动让每次 agent turn 都有可查询的本地轨迹，支撑维护与测试。

## 一、现状证据

1. 运行日志只有启动、GPU、遥测：今天的 `logs/2026-11-17.flowy.log` 为 0 字节；昨天的日志仅 6 行，无模型请求记录。
2. `flowy-obs` 现有能力是文件日志（JSON 行）、Sentry panic 上报、产品遥测 allowlist + 本地 outbox，**没有 span/trace**。
3. pi-rust 自带 `rpi-telemetry`：`TelemetryContext`、`SpanOptions`、`SpanStatus`、JSONL 的 `ProductionTelemetryContext`；但 `rpi-agent`／`rpi-ai` 中**没有任何调用点**（全仓 grep 只命中 telemetry crate 自身与文档），因此不能依赖它产出数据。
4. pi 的事件流本身已具备时间线：`AgentEvent` 覆盖 `TurnStart/TurnEnd`、`MessageStart/Update/End`、`ToolExecutionStart/End`、`RetryScheduled`、`AgentEnd`，`MessageEnd` 携带完整 `AssistantMessage`（含 usage、stop_reason）。
5. 迁移后的 `history.json` 有完整消息与模型名，但没有耗时、首 token 延迟、token 用量、取消原因。

## 二、社区调研结论

1. **OpenTelemetry GenAI semconv**：`gen_ai.*` 仍为 Development；span 覆盖 `invoke_agent`、`execute_tool`；token 命名 `gen_ai.usage.{input,output,cache_read,cache_write,reasoning.output}_tokens`；内容默认 `NO_CONTENT`，且 `gen_ai.response.status` 原生区分 `cancelled`／`incomplete`／`failed`。参考：<https://github.com/open-telemetry/semantic-conventions-genai>。
2. **OpenInference**：用 `openinference.span.kind` 枚举（`LLM/CHAIN/TOOL/AGENT/...`）对 span 分类，属性扁平、零基索引。参考：<https://github.com/Arize-ai/openinference>。
3. **Langfuse**：`trace > observation(span/generation/event)`，score 独立；短生命周期进程需显式 flush；需 PG/ClickHouse，不适合内嵌。参考：<https://langfuse.com/docs/observability/data-model>。
4. **LangSmith**：一次 turn 对应一个 trace，多轮会话用 `thread` 关联，而不是把整段会话塞进一个 trace。参考：<https://docs.langchain.com/langsmith/observability-concepts>。
5. **OpenAI Agents SDK**：层级 `trace > agent > generation/function`；敏感数据默认包含，且强调**脱敏必须发生在 exporter 内部**，不能靠外部串联。参考：<https://openai.github.io/openai-agents-python/tracing/>。
6. **Phoenix / OpenLLMetry**：前者是本地可跑的调试 UI（ELv2），后者只产 OTLP、不绑后端；两者都可作为将来导出形态参考，不引入。

## 三、决策

1. 不引入 OTel SDK、collector 或数据库；本地 JSONL 追加写，字段名对齐 `gen_ai.*`。
2. 一次 turn = 一个 trace；`conversation_id` 关联会话，`turn_index` 关联轮次。
3. 三个 span 足够：`agent.turn`、`model.request`、`tool.call`；取消用事件而非新 span。
4. 内容默认不落：只记长度与哈希；开发期用环境变量显式开启。
5. 先本地留档，远端导出后置为可选（复用 `flowy-obs` 的 allowlist/outbox 思路）。
6. pi 的 `TelemetryContext` 作为未来兼容点：若上游接线，我们实现一个 bridge 即可，现在不依赖。

## 四、数据模型

Trace（每 turn 一行）：

1. `trace_id`、`conversation_id`、`turn_index`。
2. `started_at`（wall）、`ended_at`、`duration_ms`。
3. `app_version`、`kernel_version`（pi 版本）。
4. `provider`、`request_model`、`response_model`。
5. `outcome`：`ok`／`error`／`cancelled`／`timeout`／`incomplete`。

Span（`agent.turn`／`model.request`／`tool.call`）：

1. `span_id`、`trace_id`、`parent_span_id`、`name`、`kind`（`AGENT`／`LLM`／`TOOL`）。
2. `started_at`、`duration_ms`、`ttft_ms`（仅 model.request）。
3. `status` 与 `error.type`（低基数枚举）、`error.message`（截断 512 字符）。
4. 模型：`provider`、`model`、`stream`、`finish_reason`、`response_id`。
5. token：`input/output/cache_read/cache_write/reasoning`（缺失即空，不伪造）。
6. 工具：`tool_name`、`tool_call_id`、`args_len`、`args_sha256_16`、`result_len`、`is_error`。
7. 脱敏：`content_policy`（`off`／`hash`／`full`）、`redacted_field_count`。

事件（离散点）：

1. `turn_started`、`model_first_chunk`、`tool_started`／`tool_finished`。
2. `cancel_requested`、`turn_finished`、`retry`。

## 五、采集点

1. `Engine::run_with_stream`：turn 开始/结束、outcome、取消与错误。
2. `transport` 的 `StreamFn` 包装：请求开始、首 chunk、结束/失败，算 TTFT 与总耗时。
3. `EngineEmitter`：`MessageEnd` 取 usage／finish_reason；`ToolExecutionStart/End` 生成 tool span。
4. `RunFence`／`CancelToken`：`cancel_requested` 事件与最终 `cancelled` 状态。
5. 存储路径：`{data_dir}/agent/traces/YYYY-MM-DD.jsonl`，异步 channel + 单写线程。

## 六、分阶段

P0 契约与存储（约 0.5 天，无行为变化）：

1. `core/src/trace.rs`：`TraceRecord`／`SpanRecord`／`TraceEvent`、`TraceSink` trait、脱敏工具。
2. `flowy-agent/src/trace_sink.rs`：JSONL 追加写、按日文件、14 天/50MB 上限、写失败不阻塞 turn。
3. 测试：层级与 ID、脱敏、写入失败降级、JSONL 可解析。

P1 引擎接线（约 1 天，核心价值）：

1. 四个 span 与事件接通，字段按上表。
2. `TurnObserver` 增加 usage／finish_reason 回调（最小扩展）。
3. 取消、错误、超时分别落不同 `status`。
4. 测试：真实脚本流产生完整 trace；取消产生 `cancelled`；错误产生 `error.type`；无 prompt/密钥。

P2 查询与诊断（约 0.5 天）：

1. `TraceStore::recent(conversation_id, limit)` 返回最近轨迹。
2. `flowy-agent` 暴露只读查询；UI 入口后置，先供测试与命令行排查。

P3 可选导出（后置）：

1. OTLP／PostHog 适配器放 feature 后；默认关闭。
2. 只发 allowlist 字段，`content` 永不上传。

## 七、验收标准

1. P0：`cargo test -p flowy-agent-core` 新增 trace 测试全绿；写入失败时 turn 不受影响。
2. P1：跑一次真实或脚本 turn，`traces/` 下出现一条可解析记录，包含 `model.request` 与工具 span；取消轮记录 `status=cancelled`。
3. P2：给定会话 ID 能取回最近 N 轮，按时间倒序。
4. P3：默认配置下无网络请求。

## 八、风险与对策

1. 敏感内容：默认 `off`，只落长度/哈希；开启需显式环境变量并标注 `content_policy`。
2. 磁盘增长：按日文件、单文件上限、保留 14 天，超限丢弃最旧文件。
3. 热路径：`try_send` 非阻塞，队列满即丢弃并计数；不因观测拖慢 turn。
4. 时钟：展示用 wall time，耗时用单调时钟，避免系统时间跳变。
5. provider 不返回 usage：字段留空，不做估算。

## 九、文件清单

新增：

1. `crates/flowy-agent-core/src/trace.rs`（契约、脱敏、测试）。
2. `crates/flowy-agent/src/trace_sink.rs`（JSONL 写入与轮转）。
3. 可选 `crates/flowy-obs` 增加 `agent_log_dir(data_dir)` 复用路径规则。

修改：

1. `crates/flowy-agent-core/src/engine.rs`（turn span、事件、usage 回调）。
2. `crates/flowy-agent-core/src/transport.rs`（TTFT 与请求状态）。
3. `crates/flowy-agent/src/handle.rs`（注入 sink）。
4. `crates/flowy-agent-core/src/lib.rs`、`flowy-agent/Cargo.toml`。

## 十、非目标

1. 不做 eval／scores／标注，不做完整 trace UI。
2. 不引入 OTel SDK 或 collector。
3. 不默认上传任何轨迹。
