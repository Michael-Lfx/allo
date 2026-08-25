# Flowy Cloud 会话失败调查记录

> 状态：代码修复已实施，真实 Flowy Cloud 场景验收待执行
>
> 最后维护：2026-08-25
>
> 探索分支：`fix/conversation-state-inconsistency`
>
> 基线：`e3db6b028`
>
> 当前范围：只读源码、测试和诊断路径；本记录创建前没有修改业务代码。

## 1. 问题范围与证据边界

用户反馈包含三类现象：

1. 弹出 `NOMIFUN_STATE_INCONSISTENT`，详情为：
   `turn root '<id>' owner Text conflicts with ExistingTextPlaceholder`；
2. 响应流异常中断；
3. 报错后无法删除对话。

当前调查采用以下前提：用户实际使用的是 Flowy Cloud 提供的模型和模型 key，截图中的
provider 也为 Flowy Cloud。

需要区分三种事实：

- 已经由本地源码和测试确认的状态机行为；
- Flowy Cloud 客户端适配层的实际行为；
- 仍未拿到的某一次真实失败请求的原始 SSE 和规范化事件序列。

截图只能证明用户界面收到该错误，不能单独证明 Cloud 上游返回了哪一种响应格式。

## 2. Flowy Cloud 主会话链路

当前链路可以归纳为：

```text
Flowy Cloud 登录/目录同步
  -> 内建 provider 固定 ID
  -> provider.platform = "openai"
  -> provider model profile
  -> Nomi OpenAIProvider
  -> OpenAI-compatible POST /v1/chat/completions, stream=true
  -> OpenAI SSE parser
  -> Nomi BackendOutputSink
  -> AgentStreamEvent
  -> Conversation StreamRelay
  -> 消息持久化、WebSocket、turn.completed
```

已经确认的实现事实：

- Flowy Cloud provider 使用固定的 `FLOWY_BUILTIN_PROVIDER_ID`；目录同步创建/更新 provider
  时将平台写为 `openai`，并且创建时 `model_protocols` 为 `None`。
  参见 `crates/backend/nomifun-cloud/src/provider_sync.rs`。
- `ClawModelEntry.extra` 只描述输入类型、推理能力、工具调用、上下文窗口、输出上限和
  reasoning effort；`endpoint` 与 `anthropic_endpoint` 在当前主会话解析链路中没有被用来
  选择另一套响应解析器。参见 `crates/backend/nomifun-cloud/src/flowy/types.rs`。
- provider factory 将 Cloud provider 映射到 Nomi 的 `openai` provider，并为 Cloud 增加原始
  JWT 的 `token` 头；每个 turn 还会带 `X-Flowy-Turn-Id`。参见
  `crates/backend/nomifun-ai-agent/src/factory/provider_config.rs`、
  `crates/agent/nomi-providers/src/openai.rs`。
- 因此，主会话不是 ACP 路径。Cloud 不同模型在客户端共用同一个 OpenAI 兼容 SSE 解析器，
  但 Cloud 服务端仍可能按模型返回不同的字段或事件顺序。

## 3. `NOMIFUN_STATE_INCONSISTENT` 的已证实触发机制

`StreamRelay` 对 turn 根消息维护内存中的 primary owner：

- `Unclaimed`
- `RootTextPlaceholder`
- `Thinking`
- `Text`

工具调用先于可见文本时，relay 会创建隐藏的 `text/work` 根占位消息，并把 owner 设为
`RootTextPlaceholder`。随后如果收到 `AgentStreamEvent::Text`，即使内容为空，当前代码仍会：

1. 建立 active text segment；
2. `mint_text_segment_id()` 将 owner 改为 `Text`；
3. 空内容不会真正 flush/finalize 可见文本，数据库中的占位行仍保持
   `ExistingTextPlaceholder`。

下一次 `ToolCall` 再次确保 turn root 时，owner 已经是 `Text`，数据库结果仍是
`ExistingTextPlaceholder`，兼容性检查失败并生成截图中的冲突文本。

此前使用真实 `StreamRelay` 和临时记录仓储，以如下序列复现过完全相同的诊断；临时测试已
删除，没有保留业务代码或测试改动：

```text
ToolCall(Running)
  -> Text(content="")
  -> ToolCall
  -> Finish
```

所以“共享 relay 状态机存在空文本转换缺陷”是高置信结论；但这还没有证明某次真实
Flowy Cloud 响应一定产生了该规范化事件序列。

另一个需要保留的本地入口是 robot session 的 stage-direction 过滤：它可能把原本非空的
Text 在 relay 内重写为空。因此真实上游不一定直接发送空字符串，也可能是 relay 内部重写
后触发同一状态缺陷。

## 4. Cloud 响应格式与状态错误的关系

当前 OpenAI SSE parser 对以下内容已有处理：

- `content` 文本增量；
- `reasoning_content`、`reasoning`；
- `reasoning_details` 中的文本/内容；
- OpenAI 风格的 `tool_calls` 增量和 finish reason。

其中 `delta.content` 只有在是非空字符串时才会产生 `LlmEvent::TextDelta`；
`BackendOutputSink::emit_text_delta()` 也会丢弃过滤后为空的内容，stream end 的 leftover
同样只在非空时发送。

因此目前不能把“Cloud 模型返回普通空 content”直接等同于状态不一致。更准确的关系是：

```text
Cloud 非标准响应/特殊事件顺序
  -> 如果最终被规范化为 AgentStreamEvent::Text("")
  -> 可能触发已确认的 relay owner 冲突
```

如果 Cloud 返回的是无法解析的 JSON、错误的 SSE 行、非法 UTF-8、缺少 finish reason 或
连接提前关闭，OpenAI-compatible provider 会把它作为 provider 流失败交给 Agent，当前
错误分类是 `UserLlmProviderGatewayError`，Attempt 必须失败且不能生成成功 delivery
receipt。`NOMIFUN_STREAM_BROKEN` 只表示 Agent 内部永久 event relay 断裂，不是所有 provider
SSE 解析/传输错误的统称。

## 5. 响应流异常诊断路径

OpenAI 兼容 provider 在以下失败场景会尝试写入本地失败流诊断：

```text
nomi_config::data_dir()/diagnostics/failed-provider-sse
```

诊断元数据包含 provider 类型、model、turn ID、失败原因、捕获字节数和是否截断；原始流
最多保存 256 KiB。该路径只针对解析失败、传输失败、提前结束等失败流，不会自动记录一条
完全成功但后来在 relay 中发生 owner 冲突的流。

因此：

- provider 流失败应按 model + turn ID 检查失败 SSE 元数据和原始流，并核对
  `UserLlmProviderGatewayError`、Attempt 失败和主会话汇报；
- `NOMIFUN_STREAM_BROKEN` 只在 Agent 内部 relay 断裂时检查 runtime eviction 和 relay
  诊断；
- `NOMIFUN_STATE_INCONSISTENT` 需要检查规范化事件顺序，而不能只看上游是否出现
  `content: ""`。

目前仓库内没有发现已提交的 Flowy Cloud 真实失败 SSE 样本；本轮也没有发起真实 Cloud
请求，避免在没有明确测试账号/样本边界时扩大外部影响。

## 6. 无法删除对话的独立生命周期路径

删除不是简单删除数据库行：

- durable conversation 仍为 `running` 且本进程没有精确 active turn 时，后端会以“上一进程
  的 running turn 尚无精确 process-empty 证明”拒绝删除；
- 如果当前仍有 active turn，删除会先安装 deletion guard、结束 session lifecycle，并等待
  `DELETE_CORE_GRACE`（当前为 5 秒）；超时后删除继续在后台，`conversation.listChanged(deleted)`
  才是权威成功信号；
- 如果删除目标本身是协作任务的 Attempt 子会话，后端会明确返回 409：Attempt 会话被保留为
  执行审计历史，不能直接删除。需要删除的是主/lead 会话或整个协作任务时，必须走各自的
  Execution/Conversation 生命周期，而不能把 Attempt transcript 当成普通会话删除；
- UI 当前将删除异常统一降级为通用 `deleteFailed`，不会把 409、超时、后台删除和真实删除
  失败区分展示。

因此“报错后无法删除”目前应视为会话生命周期/错误展示问题，不能直接归因于 Flowy Cloud
模型响应格式；需要用同一个 conversation ID 对齐错误时间、turn.completed、删除请求和
deleted 事件。

## 7. 当前假设排序

| 假设 | 置信度 | 当前依据 | 尚缺证据 |
| --- | --- | --- | --- |
| relay 对空 Text 的 owner 转换存在缺陷 | 高 | 本地源码 + 临时真实 relay 复现 | 需要真实 Cloud 失败 turn 的规范化事件序列 |
| 实际失败发生在协作任务的 Attempt 子会话，主会话只收到失败汇总 | 高 | AttemptRunner 创建独立 Conversation；Scheduler 失败结算后只向 lead 投影一次报告 | 需要真实 Execution/Step/Attempt 与两个 conversation ID 的对应记录 |
| 某些 Cloud 模型/网关会触发该缺陷 | 中 | 所有 Cloud 模型共用 OpenAI parser/relay；模型能力和工具顺序可能不同 | 需要原始 SSE 与规范化事件逐层对应 |
| Cloud 非标准 SSE/提前断流导致响应流异常 | 中 | parser 有明确的 malformed/EOF/transport 失败路径和失败流捕获 | 需要对应 model + turn ID 的诊断文件 |
| 删除失败是错误后的生命周期保护或后台删除超时 | 中高 | 后端有 orphan guard、5 秒删除 grace；UI 统一吞掉细节 | 需要实际 HTTP 状态、运行状态和 deleted 事件时序 |
| SOL 5.6 使用了另一套 ACP 响应协议 | 低 | Cloud provider 当前被投影为 `openai`，主会话没有走 ACP | 只有发现实际请求不是 `/chat/completions` 时才重新打开该假设 |

## 8. 下一轮只读核对计划

1. 从 Cloud 模型目录整理 model ID、`reasoning`、`tools`、`reasoning_effort`、输入模态等
   能力矩阵。
2. 对正常模型和报错模型分别对齐：请求 URL、请求模型 ID、SSE 原始 chunk、解析器事件、
   `AgentStreamEvent` 和 relay 持久化结果。
3. 优先检查包含工具调用的场景，尤其是连续工具调用、工具调用前后 reasoning/text 切换。
4. 对 provider 流错误检查失败 SSE 诊断文件和 `UserLlmProviderGatewayError`；只有确认是
   Agent 内部 relay 断裂时才检查 `NOMIFUN_STREAM_BROKEN`。对状态冲突检查是否真的存在
   `Text("")` 或 robot stage filter 的空化。
5. 用同一 conversation ID 对齐错误、turn completion、runtime release、删除请求和
   `conversation.listChanged(deleted)`。

在以上证据完成前，不选择具体修复方式。可能的修复方向（尚未批准）包括 relay 对空 Text
的状态归一、上游事件边界的统一空事件过滤，以及删除错误的生命周期状态呈现；三者不能
在没有事件证据时合并成一个“Cloud 协议兼容”修复。

## 9. 本轮验证记录

- `cargo test -p nomi-providers --test provider_openai_test test_openai_stream_empty_content_delta_skipped -- --exact`
  - 结果：`1 passed, 0 failed`。
- `cargo test -p nomifun-cloud provider_sync --lib`
  - 结果：`19 passed, 0 failed`。
- 工作区在创建本调查文档之前为干净状态；除本调查文档外没有业务代码修改。

## 10. 实测取证操作手册

### 10.1 测试前准备

建议每次使用全新的测试对话，并且一次只测试一个 Cloud model + 一个场景。不要在同一个
已经报错的对话里连续重试，否则无法区分第一次失败和后续生命周期恢复行为。

记录以下字段：

```text
test_id
开始/结束时间（含时区）
provider_id = FLOWY_BUILTIN_PROVIDER_ID
model_id（从模型选择器或 observation request 读取）
conversation_id
root_turn_id
测试场景（纯文本 / 单次只读工具 / 连续工具调用）
UI 错误 code、message、detail
删除时机和结果
```

建议使用三组最小场景：

1. 纯文本：要求“只回复 OK，不调用工具”；
2. 单次只读工具：要求读取当前工作区一个已存在的文本文件后回答；
3. 连续工具调用：要求先读取两个已存在的只读文件，再给出一句汇总。

场景 2 和 3 只使用读取操作，不要让测试模型写入、删除或执行外部副作用。

### 10.2 开启现有日志

项目文档确认日志写入数据目录下的 `logs`，也可以通过 `NOMIFUN_LOG_LEVEL` 调整过滤器。
如果从仓库启动开发后端，可在 PowerShell 中使用：

```powershell
$env:NOMIFUN_LOG_LEVEL = 'info,nomi_providers=debug,nomifun_conversation=debug,nomifun_ai_agent=debug'
bun run dev
```

调查协作任务时再加入 `nomifun_agent_execution=debug`，以看到 Attempt 创建、启动、结算和
主会话报告相关日志：

```powershell
$env:NOMIFUN_LOG_LEVEL = 'info,nomi_providers=debug,nomifun_agent_execution=debug,nomifun_conversation=debug,nomifun_ai_agent=debug'
```

如果当前使用的是已安装桌面包，不要为了测试替换现有数据目录；直接保留当次运行产生的
日志，或者使用项目实际启动方式设置同名环境变量。日志目录通常位于：

```text
<data-dir>\logs\
```

Windows 默认数据根通常是 `%LOCALAPPDATA%\NomiFun`，开发环境也可能使用
`%LOCALAPPDATA%\NomiFun-dev`；以启动日志或实际 `NOMIFUN_DATA_DIR` 为准。

`nomi_providers=debug` 只记录请求/响应摘要（model、数量、选项和响应字节长度），不再把
完整 request body 或 SSE data 行写入日志。失败 SSE 原文仅保存在本机
`diagnostics/failed-provider-sse` 的捕获文件中，可能包含用户问题、模型回答、工具参数等
敏感内容；只在本机短时间开启，收集后不要直接把捕获文件发到公共位置。

### 10.3 用开发者模式读取 canonical 证据

在设置中开启 **系统 → Developer Mode**，复现一次后打开会话页的 **观测** 面板。按
以下顺序保存证据：

1. `REQUEST`：确认 model ID、工具列表、reasoning 配置和调用类型；
2. `RESPONSE`：确认 `stop_reason`、是否有 error、是否有 response；
3. `tools`：确认工具调用数量、顺序、成功/失败和每次调用所属的 model call；
4. 回合详情：确认 `root_turn_id`、回合状态和是否缺少 terminal response。

对应的只读 HTTP API（需要登录和 Developer Mode）是：

```text
GET /api/debug/session-observations?conversation_id=<conversation_id>
GET /api/debug/session-observations/turns/<root_turn_id>?conversation_id=<conversation_id>
GET /api/debug/session-observations/turns/<root_turn_id>/calls/<model_call_id>?conversation_id=<conversation_id>
```

本地 JSONL 位于：

```text
<data-dir>\diagnostics\observation\<conversation_id>\events.jsonl
```

该观测记录的是 canonical LLM request/response 和工具执行，不会保留每个空文本增量。因此
它可以证明“哪个 Cloud model 在哪个 turn 的 response/工具链失败”，但不能单独证明 relay
曾经收到 `AgentStreamEvent::Text("")`。

### 10.4 采集失败 SSE

若现象是 `UserLlmProviderGatewayError` 或确实是 provider 流失败，优先查看：

```text
<data-dir>\diagnostics\failed-provider-sse\
```

先只读取最新的 `.json` 元数据，不要立即打开 `.sse` 原文：

```powershell
$dataDir = "$env:LOCALAPPDATA\NomiFun"
# 若当前环境使用 NomiFun-dev 或 NOMIFUN_DATA_DIR，请把上一行替换为实际数据目录
$failedDir = Join-Path $dataDir 'diagnostics\failed-provider-sse'
Get-ChildItem $failedDir -File |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 10 FullName, LastWriteTime, Length
```

元数据应至少核对：`model`、`turn_id`、`failure_reason`、`captured_bytes`、`truncated`。
只有元数据与失败回合匹配后，才在本地查看对应 `.sse`，检查：

- 是否返回非 JSON 或非法 UTF-8；
- 是否出现非标准 SSE 行；
- 是否在 `finish_reason` / `[DONE]` 前断流；
- 是否有 provider error frame；
- 是否是 tool call 参数不完整。

### 10.5 从日志中对齐一次失败

在对应时间窗口内检索这些事件：

```text
Sending OpenAI-compatible SSE request
sse chunk received
persisted failed OpenAI-compatible SSE diagnostic
NOMIFUN_STATE_INCONSISTENT
UserLlmProviderGatewayError
NOMIFUN_STREAM_BROKEN（仅内部 relay 断裂时）
conversation deletion
Conversation deleted
```

对齐规则是：`model_id + root_turn_id + 时间窗口` 三者至少匹配两个，不能只凭模型名称
判断同一请求。

### 10.6 删除行为的单独验证

在一次模型失败后分别执行：

1. 错误卡片出现后立即删除；
2. 等待 10 秒后删除；
3. 观察是否收到 `conversation.listChanged(deleted)`；
4. 刷新会话列表并重新查询该 conversation 是否仍存在。

记录每次是：HTTP 409（未证明的 running/orphan）、超时（后台删除）、普通失败，还是收到
deleted 事件后 UI 没有移除。删除成功的判据是持久化删除 + `deleted` 事件，不是 HTTP 请求
是否在 5 秒内返回。

### 10.7 证据判定

| 观察结果 | 能证明什么 | 不能证明什么 |
| --- | --- | --- |
| failed SSE 元数据匹配 model/turn，原始流提前结束或格式非法 | Cloud 传输/响应协议导致 provider gateway error、Attempt 失败的证据 | 不能证明内部 relay owner 冲突，也不应直接命名为 `NOMIFUN_STREAM_BROKEN` |
| canonical response 有 tool 调用但无 terminal response | 该 model call 在 provider/engine 边界未正常结束 | 不能证明存在空 `AgentStreamEvent::Text` |
| UI 详情为 `owner Text conflicts with ExistingTextPlaceholder` | relay 的 owner 检查失败 | 不能单凭此判断 Cloud 原始响应格式 |
| raw SSE 里出现 `content: ""` | Cloud 发过空 content frame | 当前 parser 会过滤它，仍不能证明它进入了 relay |
| 删除最终收到 `conversation.listChanged(deleted)` | 删除最终完成 | 不能说明之前的 UI 错误展示正确 |

如果以上证据仍无法回答“空 Text 是在哪里产生的”，才需要申请一个短期诊断性改动：只在
`BackendOutputSink`/`StreamRelay` 记录 `event kind + content length + owner + root id`，不记录
正文和 key，复现后立即移除。该诊断改动属于后续单独审查项，本阶段尚未实施。

## 11. 新增场景：主会话简短输出，但协作 Attempt 失败

### 11.1 对用户描述的代码级映射

用户所说的“协助进程”目前最符合产品里的 **协作任务（AgentExecution）**，而不是普通的
“召唤伙伴”会话或 ACP 会话。依据是 UI 的入口和状态词使用“协作任务 / 协作进度 / 协作模型”，
而执行架构明确把一次协作拆成 `Participant -> Step -> Attempt`。

在“先理解一个小项目结构”的场景中，实际链路是：

```text
主/lead Conversation
  -> AgentExecution
    -> Planner 生成一个或多个 Step
      -> Scheduler 创建 ExecutionAttempt
        -> AttemptRunner 创建独立的 Nomi Conversation
          -> Cloud provider 的 OpenAI-compatible SSE
          -> Agent runtime / BackendOutputSink / StreamRelay
        -> Attempt 成功或失败结算
  -> Scheduler 生成 Execution 终态摘要
  -> 将一次 agent_execution_report 投影回主/lead Conversation
```

源码核对结果：

- `ConversationAttemptRunner::execute` 从不可变的 `ExecutionParticipant` 读取明确的
  `provider_id + model`，为 Attempt 创建独立 `CreateConversationRequest`，类型为 `Nomi`；
- 创建完成后，Scheduler 先把这个 conversation ID 写入 Attempt Link，再通过受信任的
  `AgentExecutionConversationPort` 投递 turn；这条 port 最终仍调用
  `ConversationService::send_agent_execution_message_idempotent`；
- `AttemptOutcome` 只携带子会话的 `conversation_id`、文本、文件、`ok` 和 token；缺少精确
  delivery receipt、provider error 或超时都会令 Attempt 不能成功完成；
- Scheduler 会把失败原因写入 Attempt 的 `error`，将 Step/Execution 归约为 failed 或
  completed_with_failures；终态后 `report_lead` 只把持久化 summary 写成主会话的一条
  `agent_execution_report` assistant message，不会再次调用主模型生成长回复。

所以“主进程只做简短输出”并不能证明主模型完整执行了子任务。它可能只是：

1. 主会话负责创建/展示协作任务；
2. 独立 Attempt 子会话在 Cloud SSE、Agent runtime 或 relay 阶段失败；
3. Scheduler 把失败状态/短摘要投影回主会话。

### 11.2 对 Flowy Cloud 模型结论的修正

“用户一直使用 Flowy Cloud 的 key”仍然成立，但要区分 provider、model 和会话：

- Attempt 不从主会话当前运行时猜模型，而是使用被分配 Participant 快照中的
  `provider_id + model`；因此要从 **Attempt 子会话** 的 conversation model、Execution detail
  的 participant/effective config 或 observation request 读取实际 model ID；
- 如果协作任务选择“单一模型”，且只有一个 Flowy Cloud Participant，主会话和 Attempt 通常
  会落到同一个 Cloud provider/model；
- 如果选择“自动选择/模型范围”、存在多个 Cloud 模型，provider 可以相同但 Attempt 实际
  model 可能不同。不能仅凭主会话顶部的模型名给所有失败归因到 SOL 5.6；
- 无论主会话还是 Attempt，只要其 Conversation 类型为 Nomi，当前都复用同一套 Cloud
  `openai` provider、OpenAI SSE parser、Agent runtime 和 StreamRelay。故问题更可能是
  “某个 Attempt 的具体响应/事件序列触发了共用路径”，而不是协作任务有另一套 Cloud 协议。

当前最重要的证据对象不是单一主 `root_turn_id`，而是这组关联：

```text
lead conversation_id
  -> execution_id
    -> step_id
      -> attempt_id
        -> attempt conversation_id
          -> attempt root_turn_id / model_call_id / model_id
```

### 11.3 重新定义“失败证据”

实测时必须同时检查主会话和失败 Attempt，不能只打开主会话的 Session Observation：

| 对象 | 需要记录 | 目的 |
| --- | --- | --- |
| 主/lead Conversation | `conversation_id`、主 turn/root、最终短消息内容、Execution ID | 证明主会话只是收到报告，还是它本身也失败 |
| AgentExecution | `execution_id`、Execution status、Step status、failure policy、summary | 证明失败发生在调度聚合层还是仅 UI 显示层 |
| ExecutionAttempt | `step_id`、`attempt_id`、`attempt_no`、`conversation_id`、participant、error、retry | 定位真正失败的协作者及是否发生重试 |
| Attempt Conversation | 子会话 model、子 turn/root、消息错误、Session Observation | 对齐 Cloud 请求和 relay 的真实失败边界 |
| failed-provider-sse | `model`、`turn_id`、failure reason、时间 | 证明具体 Cloud 请求是否在 SSE/传输层失败 |

UI 的协作 Step inspector 已经会根据 `attempt.conversation_id` 加载只读的 Attempt transcript；
该 transcript 可以用于人工确认“失败发生在协作者”，但完整取证仍要结合 Execution detail 和
子会话 observation。Attempt transcript 不是普通可删除会话，后端的 409 是设计上的审计保留，
不能把它当成删除失败的根因。

### 11.4 针对该场景的最小复现实验

用新建的协作任务测试，不要在已坏的主会话上继续重试：

1. 在“协作模型”中先选 **单一 Flowy Cloud 模型**，把模型 ID 记为 `M`；这样先排除自动路由
   把不同 Cloud model 混入结果的变量；
2. 工作目录指向一个小型、只读可访问的项目；目标固定为“只理解并总结目录结构，不修改文件”；
3. 记录主 `conversation_id` 和新建的 `execution_id`；
4. 执行后在协作面板展开失败 Step，记录 `attempt_id`、Attempt 的 `conversation_id`、实际
   participant/model；如果有 retry，要把每个 attempt 分开记录；
5. 打开该 Attempt transcript 和 observation，另外查看主会话 observation；
6. 将两边的 `model_id + root_turn_id + 时间` 与日志、failed-provider-sse 元数据对齐；
7. 最后分别验证：删除主会话；尝试删除 Attempt 子会话并记录预期的 409；等待终态后刷新列表，
   观察主会话是否收到 `conversation.listChanged(deleted)`。

判定方式：

- 子会话有 `llm/request`，但没有对应 `llm/response`，且有 failed SSE 或 runtime/receipt
  超时：优先归为 Attempt 的 provider/传输/终态交付失败；
- 子会话 observation 有完整 response，但日志出现 `NOMIFUN_STATE_INCONSISTENT`，且错误中
  的 root ID 属于子会话：归为共享 StreamRelay 状态机在 Attempt 路径触发，仍需继续寻找
  `AgentStreamEvent::Text("")` 的来源；
- 子会话已明确 failed，主会话只有 `agent_execution_report` 短消息：这是“子失败被主摘要化”
  的证据，不是 SOL 5.6 响应格式已经被证明不兼容；
- 主会话和子会话都失败，且两个 root/turn 各自都有证据：说明可能存在两个独立故障，不能
  用主会话的短消息把它们合并为一个 Cloud 协议问题；
- 只有主会话失败、没有 Attempt 创建或子会话：才回到普通主会话链路调查，不能套用本节结论。

本节把当前结论从“Cloud 响应格式可能触发 relay”收窄为：**先证明失败发生在哪个
Conversation，再判断该 Conversation 的 Cloud SSE 是否触发共用 relay 缺陷。** 在拿到上述
关联证据前，不选择修复方案，也不修改业务代码。

## 12. 新增实测：多子任务打开协作栏后 ReactFlow 页面渲染崩溃

### 12.1 用户实测证据

用户发送“创建多个子任务进行搜索”的请求后，右侧协作任务栏刚打开，页面立即显示：

```text
页面渲染出错（已被路由错误边界捕获，未影响其它页面）
TypeError: Cannot read properties of null (reading 'useState')
```

截图中的关键调用链是：

```text
exports.useState
  -> useColorModeClass (@xyflow/react)
    -> ReactFlow (@xyflow/react)
      -> DagCanvas.tsx
        -> ExecutionTopPanel.tsx
          -> WorkspaceRailBody / ChatWorkspace / ChatSlider
```

这是一条前端同步渲染调用链。截图中没有 Cloud provider、OpenAI-compatible SSE、
`BackendOutputSink`、`StreamRelay` 或 AttemptRunner 的栈帧，因此它首先证明的是“协作栏的
ReactFlow 画布挂载失败”，而不是 Cloud 返回内容的格式错误。

### 12.2 与“多子任务”场景的代码级对应

当前实现的触发路径与用户描述能够对应起来：

```text
主会话创建 AgentExecution
  -> executionId 出现
    -> ExecutionConversationLayout 自动打开协作预览页签
      -> ExecutionTopPanel（lazy）挂载 DagCanvas
        -> detail 返回且 activeSteps.length > 0
          -> DagCanvas 首次渲染 ReactFlow
            -> @xyflow/react 的 useColorModeClass 调用 useState 时崩溃
```

源码核对结果：

- `ExecutionConversationLayout` 在 `execution.executionId` 出现后调用
  `dispatchWorkspaceOpenPreviewTool`，并把 `ExecutionTopPanel embedded` 放进协作页签；
- `ExecutionTopPanel` 使用 `React.lazy(() => import('./DagCanvas'))`，通过 `Suspense` 挂载
  画布；
- `DagCanvas` 在 `detail` 存在但没有有效 Step 时只显示“正在准备协作计划”，只有
  `activeSteps.length > 0` 才渲染 `<ReactFlow>`；
- `<ReactFlow>` 传入了 `colorMode={theme}`，截图的 `useColorModeClass` 正是该组件内部的
  运行路径；
- 主 renderer 入口使用一个 `createRoot(document.getElementById('root'))`。目前没有发现
  协作栏通过 portal 或另一个 React root 单独挂载；`workshop/editor/index.tsx` 的第二个
  `createRoot` 属于独立图片编辑器，不在该会话协作栏路径上。

因此，多子任务本身更像是让画布更快进入 ReactFlow 分支的触发条件；它不等于“多子任务的
Cloud 响应格式导致 React 的 useState 为空”。即使 Attempt 后续失败，前端也可能在显示
失败 Step 之前先因 ReactFlow 崩溃，造成用户看到“发送后马上页面报错”的表象。

### 12.3 依赖与运行时核对结果

当前声明和本地安装状态如下：

| 项目 | 结果 | 证据含义 |
| --- | --- | --- |
| `ui/package.json` React | `^19.1.0` | 声明允许 React 19.1.x 及后续兼容版本 |
| `ui/package.json` ReactDOM | `^19.1.0` | 与 React 使用同一大版本范围 |
| `ui/package.json` ReactFlow | `^12.11.1` | 画布依赖声明 |
| 当前 Vite 预构建 React | `19.2.7` | `ui/node_modules/.vite/deps/_metadata.json` 指向该包 |
| 当前 Vite 预构建 ReactFlow | `12.11.1` | 同一份元数据指向 `@xyflow/react` 12.11.1 |
| 当前 Vite browser hash | `f765dc30` | 与截图 URL 中 React chunk 的 query 一致 |
| ReactFlow peer dependency | React/ReactDOM `>=17` | 包元数据本身没有证明 React 19 不受支持 |

当前预构建元数据把应用 React 和 ReactFlow 的依赖都解析到同一套 React 19.2.7，因此目前
不能把“重复 React dispatcher”写成已确认根因。不过 Bun store 中同时存在 React 19.2.8、
`@xyflow/react` 12.11.2 等其他包目录，而当前项目锁定/链接的是 React 19.2.7 和 ReactFlow
12.11.1；这保留了“旧 Vite 预构建缓存或安装链接在某次运行中混入另一套 React”的可证伪
假设，不能当作现有截图已经证明的事实。

### 12.4 关系判定：相关，但不是同一层故障

现阶段应把两类错误画成并行关系，而不是串成“Cloud 响应直接导致页面 React 崩溃”：

```text
Cloud / Attempt 后端链路可能失败
  -> Execution detail / Step 状态更新
    -> 协作栏打开并尝试展示画布
      -> ReactFlow 运行时挂载失败
        -> 路由错误边界显示页面渲染错误
```

这说明它们在产品流程上相关：同一个 AgentExecution 会同时触发 Attempt 后端执行和协作
栏 UI 展示。前端崩溃还会遮蔽真实的 Step/Attempt 错误，使用户难以判断此前的“响应流异常”
和“会话状态不一致”究竟发生在哪个会话。

但截图没有证明以下任一命题：

1. Flowy Cloud 返回了不兼容的 JSON/SSE；
2. SOL 5.6 或其他某个 Cloud model 的响应触发了 `useState` 为空；
3. `TextPlaceholder` owner 冲突和 ReactFlow 崩溃共享同一个根因；
4. Attempt 已经失败，或者失败发生在主会话而不是子会话。

### 12.5 更新后的假设与可证伪预测

按当前证据排序：

| 优先级 | 假设 | 若成立，应观察到 |
| --- | --- | --- |
| 1 | 某次运行加载了重复/错配 React，或复用了过期 Vite 预构建缓存 | 不依赖 Cloud 数据，静态/已有 Execution 只要挂载 DagCanvas 就能复现；清理并重建 Vite 依赖后结果改变 |
| 2 | ReactFlow 12.11.1 与实际 React 19.2.x 运行时组合存在未覆盖的兼容问题 | 任何有效 ReactFlow 挂载都出现同一 `useColorModeClass -> useState` 错误，而不是只在某种 Cloud 响应下出现 |
| 3 | 协作栏首次挂载、Suspense 或 provider 边界的时序问题 | `detail` 为空时只显示准备中；第一次收到有效 `activeSteps` 后才崩溃；不要求特定 provider |
| 4 | `DagCanvas` 的 Step/edge 数据形状触发画布分支中的其他错误 | 替换为最小静态 detail 后不崩溃，只有某个 Step/edge 组合触发；但这不能自然解释当前 `useState` dispatcher 为 null |
| 5 | 前端 ReactFlow 崩溃与后端 Attempt 失败只是同时发生 | 独立查看 Execution detail/Attempt observation 能找到后端失败；用本地静态画布仍能复现前端错误 |

### 12.6 当前验证边界和下一步取证

本轮已经完成源代码、依赖声明和 Vite 预构建元数据的只读核对，但没有声称在本地完整
复现了用户截图：

- Vite + `dev:web` 已经成功启动，后端首次编译约 5 分钟后监听 `127.0.0.1:8787`；
- 浏览器可以进入 Flowy 页面，但当前选中的会话没有一个可安全复用的、正在产生多 Step 的
  Execution，且本轮没有代表用户向 Cloud 发起新的请求；
- 浏览器中此前仅启动 UI、没有后端时留下的 `/api` 代理错误不能用于判断 ReactFlow；
- 当前仓库找到的是结构字符串测试，没有发现真正挂载 `DagCanvas`/`ReactFlow` 的回归测试，
  这解释了为什么 typecheck 或结构测试可以通过而运行时 hook dispatcher 仍可能崩溃。

后续实测应分开记录两组证据：

1. **前端证据**：新会话触发 Execution 后，分别记录协作栏刚打开、`detail` 为空、首次出现
   有效 Step 三个时间点；保存浏览器控制台完整栈、Vite `browserHash` 和实际加载的 React/
   ReactFlow 版本。若能用已有 Execution 或最小静态 detail 复现，则可先排除 Cloud。
2. **后端证据**：按第 11 节的 `lead conversation -> execution -> step -> attempt ->
   attempt conversation -> root/model_call` 关联，单独保存 Attempt 状态、子会话 observation、
   failed-provider-sse 元数据和主会话摘要；不要用页面渲染错误代替 Attempt 失败证据。

以上段落保留为实施前的调查快照；代码修复和当前验证边界见第 13 节。ReactFlow 仍仅作为
依赖安装/缓存问题的环境前置条件复核，不纳入本次 Cloud 流、relay 或删除契约修复。

### 12.7 后续实测对该问题的降级结论

用户补充：此前出现 ReactFlow 报错的环境可能尚未执行 `bun install`，随后重新实测同一类
协作操作已经不再出现该 React 错误。

这是一条重要的差分证据，当前应把 ReactFlow 问题从“待处理产品故障”降级为“环境依赖/旧
Vite 预构建缓存导致的历史性现象”：

- 它支持“依赖未安装完整、包链接不一致或旧预构建缓存”假设；
- 它削弱“ReactFlow 与 Flowy Cloud 响应内容存在确定因果关系”的假设；
- 它不构成严格 A/B 证明，因为目前没有保存安装前后的 lockfile、实际包版本、Vite cache
  和完整控制台日志；
- 暂不应仅凭这张截图修改 ReactFlow、Cloud parser、StreamRelay 或协作状态机。

若以后再次出现同一错误，优先记录并比较：`bun install` 是否完成、`bun.lock` 是否变化、
React/ReactDOM/ReactFlow 实际版本、`ui/node_modules/.vite/deps/_metadata.json` 的
`browserHash`，以及清理预构建缓存前后的浏览器完整调用栈。只有在依赖安装完成且同一场景
仍稳定复现，才重新提升本节前端假设的优先级。

## 13. 已实施的代码修复与验证边界

### 13.1 StreamRelay 空文本状态修复

在 `StreamRelay` 的规范化 `AgentStreamEvent::Text` 分支增加了零长度内容短路。空文本不再：

- 完成 thinking 状态；
- 设置 `emitted_response`；
- 同步或重新认领 turn root；
- 创建文本 segment；
- 广播或持久化消息。

该修复位于 relay 边界，不区分 Flowy Cloud、SOL 5.6 或其他模型。新增回归覆盖了：

1. `ToolCall -> Text("") -> ToolCall -> Finish`；
2. 机器人阶段指令过滤为空后继续工具调用；
3. 空格文本仍然广播并持久化。

### 13.2 响应流和协助汇报修复

- 保留 OpenAI-compatible parser 对空 delta 的既有过滤，不增加 SOL 5.6 特判；
- provider 请求/响应调试日志不再打印完整 body 或 `data` 内容，只保留安全摘要和响应长度；
- provider 的 malformed/invalid UTF-8/EOF/缺少 finish reason 统一保持 provider gateway error
  语义，不冒充内部 `NOMIFUN_STREAM_BROKEN`；
- 新增非法 UTF-8 和缺少 `finish_reason` 的失败 SSE 捕获测试；
- 协助终态仍遵守“没有 delivery receipt 不能成功”的规则；
- `aggregate_summary` 在没有 `output_summary` 时回退到 Attempt 的 `error`，避免主会话只显示 `step | failed | -` 而丢失流异常原因。

### 13.3 删除错误契约和 UI

保留 Attempt 审计记录和 running orphan 的删除保护，新增稳定错误码：

| 场景 | 错误码 |
| --- | --- |
| Attempt 子会话保留为审计记录 | `CONVERSATION_ATTEMPT_RETAINED` |
| running 状态缺少进程为空证明 | `CONVERSATION_RUNNING_ORPHAN` |
| 删除已转入后台 | `CONVERSATION_DELETE_PENDING` |

错误响应增加 `retryable`、`requires_recovery`、`authoritative_event` 等结构化详情。前端
单删按错误码展示明确中英文提示；批量删除使用 `Promise.allSettled`，成功项不会被失败项
掩盖，失败项保持在列表中并按原因汇总提示。失败请求不会伪造 `conversation.deleted`，
最终成功仍以 `conversation.listChanged(deleted)` 为准。

### 13.4 已执行的自动化验证

已通过：

- `cargo test -p nomifun-conversation --lib empty_text`；
- `cargo test -p nomifun-conversation --lib tool_first_uses_hidden_text_placeholder_until_final_text`；
- `cargo test -p nomi-providers --lib sse_`；
- `cargo test -p nomi-providers --lib eof_before_finish_reason`；
- `cargo test -p nomifun-common --lib error::tests`（新增 HTTP 响应体测试前的既有 16 项通过）；
- `cargo test -p nomifun-common stage_direction`（13 passed）；
- `cargo test -p nomifun-agent-execution --lib aggregate_summary_preserves_stream_failure_reason`；
- `cargo test -p nomifun-agent-execution --lib failed_or_textless_agent_outcome_can_never_complete`；
- `cargo test -p nomifun-conversation --lib cold_running_orphan_rejects_user_stop_and_delete_without_mutation`；
- `bun test --cwd ui src/renderer/pages/conversation/SessionList/hooks/conversationDeleteErrors.test.ts`；
- 前端删除结构/分类测试合计 5 passed；
- `bun run check:i18n`、`bun run check:error-surface-contract`；
- `cargo fmt --all -- --check`、`git diff --check`。

`cargo test -p nomi-providers --test provider_openai_test` 已三次尝试执行（其中一次使用 C 盘临时
Cargo target），但 workspace 编译阶段仍因 D 盘剩余空间不足（`os error 112`，`No space left
on device`）无法写入 `build.noindex`，因此本轮不能把该集成测试宣称为最终通过；provider 的
SSE 单元/捕获测试已通过。新增的 `conversation_delete_response_body_exposes_stable_contract`
已在 C 盘临时 Cargo target 中随 common 错误测试通过，`17 passed, 0 failed`。

`cargo test -p nomifun-conversation --lib delete_rejects_soft_deleted_execution_attempt_transcript`
仍被 Windows `runtime-locks` authority file 的拒绝访问拦截，失败发生在测试夹具初始化，
没有到达删除断言，因此不能视为产品行为失败。

`bun run typecheck` 被仓库已有的 Markdown、Preview、视频画布和其他组件类型错误拦截；
输出中没有新增删除错误分类文件或 `useConversationActions.ts` 的类型错误。完整 `bun run check`
和 `cargo check --workspace` 仍需在清理这些环境/基线问题后再作为发布门禁执行。本轮实际
启动 `cargo check --workspace` 时，D 盘仅剩约 84 KiB，最终因 `build.noindex` 写入
`os error 112`（磁盘空间不足）中断；这属于验证环境阻塞，不是本次代码编译错误。

### 13.5 尚待真实验收的内容

本轮没有调用真实 Flowy Cloud，也没有声称 SOL 5.6 或其他 Cloud model 已完成线上验证。仍需
使用单一 Flowy Cloud 模型实测“创建多个子任务进行搜索”，记录 lead、execution、step、
attempt、子会话和 turn ID，确认：协作栏不再触发会话状态不一致；真实流异常能落入明确失败
路径；子任务失败会在主会话摘要中保留原因；主会话删除、Attempt 审计保留、后台删除和
orphan 保护分别符合上述契约。ReactFlow 仍只以完成 `bun install` 为前置条件复核，不属于
本次代码修复范围。
