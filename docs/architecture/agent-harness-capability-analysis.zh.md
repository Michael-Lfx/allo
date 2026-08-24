# Flowy Agent Harness 能力、性能与优化审查

> **最后维护：** 2026-08-24（元数据维护，未重写结论）· 核对基准：commit `d791691c6` ·
> 文档性质：能力/性能审查记录（时点快照）· 内容基线：2026-08-06
>
> 这里的主项目是当前工作区 `C:\code\flowy\allo`，项目名为 Flowy。本文审查的是
> Flowy 的 Agent Harness 是本文唯一的主审查对象。外部项目只在最后作为概念参考，
> 不能替代本仓库源码证据。
>
> 证据范围：`crates/agent/nomi-agent` 的 `AgentEngine`，
> `crates/backend/nomifun-ai-agent` 的宿主组装，以及
> `crates/backend/nomifun-agent-execution` 的跨会话执行控制。

## 结论

当前 Flowy 的 Agent Harness 已经是一个完整的模型-工具执行循环，短板不在“有没有
loop”，而在以下三个层次：

1. **能力正确性（P0）**：普通任务的“完成”仍主要由模型自然返回 `EndTurn` 决定。
   系统提示要求模型在结束前验证，但默认没有统一的运行时 completion/evidence gate。
   Goal Judge 可以补上这一层，但它是显式启用的 per-session 能力，不是普通会话默认行为。
2. **失败恢复（P1）**：已有 schema retry、stagnation nudge 和硬上限，能防止失控，
   但运行时工具失败没有统一的失败分类和替代策略，恢复更多依赖下一次模型自行判断。
3. **本地热路径（P1）**：ContextContributor 串行读取、每轮复制消息、同步保存完整会话和
   index，可能把本地准备时间混进用户感知的首 token/下一轮延迟。

模型缓存是这些问题的支撑因素，不是本文主线。当前代码已经把稳定 system prompt 与动态
turn tail 分开，并记录 cache read/create usage；优化时不应把所有 Agent 能力问题归因于
provider 缓存。

## 1. 当前架构边界

```text
nomifun-ai-agent (宿主组装、权限、会话入口)
        |
        v
nomi-agent::AgentEngine
  ├─ provider stream
  ├─ tool authority / schema / approval / execution
  ├─ plan mode / steering / context contributors
  ├─ compaction / stagnation guard / tool retry
  └─ session transcript checkpoint

nomifun-agent-execution::AgentExecutionEngine
  └─ 跨会话 execution、step、attempt、租约、调度和重试
```

这两个层次已经存在，不能把跨会话调度状态再塞回 `AgentEngine`，也不能用外部项目的
控制面职责定义 Flowy 的单轮 Agent 能力。

| 层 | 当前实现 | 责任 |
| --- | --- | --- |
| 单轮 Agent Harness | `crates/agent/nomi-agent/src/engine/mod.rs` | provider 流、工具轮次、上下文、终止与错误处理 |
| 宿主能力注入 | `crates/backend/nomifun-ai-agent/src/manager/nomi/agent.rs` | provider、工具、memory、knowledge、skills、goal、browser、审批组装 |
| 长程执行控制 | `crates/backend/nomifun-agent-execution/src/engine.rs` | execution/step/attempt 持久化、调度、取消、重试和 Agent 间委派 |

## 2. 已有能力与代码证据

| 能力 | 当前事实 | 代码证据 |
| --- | --- | --- |
| 工具权限边界 | 每次 provider 请求先从当前工具定义生成 `ProviderToolAuthority`，dispatch 不使用隐式全量 registry | `engine/mod.rs:1319-1334` |
| provider 流协议 | 校验 `Done` 数量、tool id、工具是否被 advertise、参数对象、preview 与完整 ToolUse 的对应关系 | `engine/mod.rs:1451-1719` |
| 工具安全执行 | 支持 schema 校验、审批、工具结果和 artifact media delivery；媒体投递失败会把结果转为错误，迫使模型重试 | `engine/mod.rs:1747-1778`, `1962-2064` |
| 工具并发 | 工具层按 `is_concurrency_safe` 等条件分批并发，同时保留结果顺序和错误屏障 | `crates/agent/nomi-agent/src/tool_execution.rs` |
| 长上下文 | 每次 API 前执行 microcompact、autocompact 和 emergency gate | `engine/mod.rs:2273-2360` |
| 停滞保护 | 相同工具结果和 all-failed turns 进入 nudge/abort 状态机；另有默认安全 `max_turns` | `engine/mod.rs:1284-1296`, `2085-2177` |
| 工具 schema retry | `ToolRetryTracker` 只对相邻轮次中明确的 pre-dispatch schema failure 建立 retry lineage | `engine/mod.rs:72-187`, `2067-2072` |
| steering | 运行中用户插话进入 inbox，在工具结果或自然结束边界被吸收，不需要重启整个 turn | `engine/mod.rs:551-562`, `1847-1869`, `2117-2167` |
| plan mode | 进入 plan mode 后仅向 provider 暴露 Info 工具，退出后恢复原 allow-list | `engine/mod.rs:1319-1329`, `2400-2412` |
| goal judge | goal 是 opt-in；自然 `EndTurn` 后才调用 judge，judge 可返回 done/continue/wait，并有 parse/transport breaker | `bootstrap.rs:408-412`, `954-956`, `engine/mod.rs:1872-1905`, `goal/runtime.rs:210-297` |
| context contributor | host 可注册 memory、knowledge、skill 等动态来源，注入最后一条 user message 的 turn tail | `context_contributor.rs:1-12`, `engine/mod.rs:1347-1389` |
| 会话恢复 | session 保存消息、usage、deferred tool identity 和 editable turn；宿主可恢复 goal snapshot | `engine/mod.rs:2419-2434`, `bootstrap.rs:833-846` |
| 跨会话执行 | `AgentExecutionEngine` 外置 scheduler、attempt runner 和 repository，不依赖单轮 engine 的循环状态 | `nomifun-agent-execution/src/engine.rs:176-210` |

## 3. 需要优化的地方

### P0-A：普通任务没有统一的完成证据闸门

**当前现状**

`AgentEngine::new_with_provider` 和 `resume_with_provider` 默认把 `goal` 设为 `None`。
`AgentBootstrap::goal` 也明确是 opt-in，只有宿主传入 `GoalSpec` 时才调用 `set_goal`。
在 `execute_turn_inner` 中，只要 provider 返回无工具的正常 `EndTurn`，且没有 steering，
就直接保存并返回 `AgentResult`；Goal Judge 只在 `self.goal` 存在时运行
（`engine/mod.rs:1840-1905`）。

系统提示确实要求“修改后运行构建和测试再报告完成”（`context.rs:109-117`），但这是
模型可被忽略的 prompt 约束，不是运行时事实检查。工具结果里的 artifact delivery 也只
保证结果已交付，不会自动判断用户目标是否已经满足。

**为什么不合适**

对“改代码并通过测试”“生成文件”“完成浏览器操作”这类任务，模型可以在没有执行验证的
情况下返回一段看起来完整的文本，Harness 仍会把它当成正常结束。这是 Agent 能力缺口，
不是 provider 速度问题；仅增加 eval 数量只能暴露问题，不能修复完成语义。

**怎么优化**

在宿主到 engine 的任务配置中增加一个可选 `CompletionPolicy`，至少包含：

- `Conversation`：普通问答保持当前低延迟行为；
- `EvidenceRequired`：自然 `EndTurn` 前必须有结构化工具 receipt、artifact locator 或
  指定命令成功结果；
- `GoalJudged`：复用现有 Goal Judge，并把 evidence receipt 一起提供给 judge。

完成闸门应读取 `OutputSink`/工具执行产生的结构化记录，而不是只匹配最后一段文本。
证据不足时追加一条明确的 continuation，让模型执行验证；达到预算仍不足时返回
`Incomplete/Blocked`，不能伪装成成功。代码位置建议放在 `AgentEngine` 的自然终止边界
和 `nomifun-ai-agent` 的任务配置之间，不要把所有普通聊天都强制变成 judge turn。

**为什么这样优化**

它把“模型说完成”和“任务状态已被验证”分开，同时保留普通对话的低延迟路径。现有
artifact media delivery、tool result 和 Goal contract 可以复用，不需要改 provider 协议。

**验收**

- 代码修改但未运行验证命令时，`EvidenceRequired` 不得返回成功；
- 有成功命令输出或已验证 artifact locator 时才能通过；
- 普通 `Conversation` 行为和响应格式不变；
- provider、工具、审批错误仍按原有错误语义返回，不能被 completion gate 吞掉。

### P1-A：工具失败有保护，但缺少可执行的恢复策略

**当前现状**

`ToolRetryTracker` 的注释和实现都限定为“相邻轮次、明确的 pre-dispatch schema failure”。
运行时错误则由 `is_error`、相同 outcome signature 和 all-failed counter 交给
`StagnationGuard`；达到阈值后先注入 nudge，再 abort（`engine/mod.rs:2085-2175`）。

**为什么不合适**

这能阻止无限循环，却不能区分“参数错”“权限拒绝”“超时”“资源不存在”“外部服务暂时
失败”。不同错误应该采取不同动作；当前模型只能从自然语言错误文本中猜下一步，容易重复
同一个失败调用或过早放弃。

**怎么优化**

定义内部 `ToolFailureContext`，由工具执行层填入稳定类别、是否可重试、推荐替代动作、
已尝试次数和相关 artifact。下一轮给模型的是结构化 recovery hint：例如 schema failure
要求修参，权限错误要求请求批准，not-found 要求先探测，timeout 要求轮询或换策略。
同一 retry group 继续保留硬上限；只有 `retryable=true` 才允许自动重试。

**为什么这样优化**

把“防死循环”升级为“有方向的恢复”，不会放宽当前 authority、schema 或审批边界；同时
能减少无意义 provider 轮次和工具调用。

**验收**

- 每类错误都能在事件和 transcript 中保持稳定分类；
- 不可重试错误不再自动重复；
- 可重试错误最多按策略重试，并在耗尽后返回可解释的 blocked 状态；
- 现有停滞保护测试继续覆盖交替失败和相同签名失败。

### P1-B：plan mode 是安全过滤，不是可审计的计划协议

**当前现状**

plan mode 的运行时状态只有 `is_active` 和进入前的 allow-list。进入后过滤为 Info 工具，
提示词要求模型把计划写在响应中；`ExitPlanMode` 的 `PlanModeTransition` 当前携带
`plan_content: None`（`plan/state.rs`、`plan/prompt.rs`、`plan/tools.rs:153-166`）。

**为什么不合适**

它能防止计划阶段写文件，但宿主没有拿到一个结构化、版本化、可批准的计划对象，也不能
在执行阶段检查“实际改动是否符合计划”。复杂任务因此仍靠模型记忆计划文本。

**怎么优化**

增加 `PlanArtifact`（目标、步骤、范围、验证命令、风险和版本），由 `ExitPlanMode` 提交；
宿主批准后把 artifact id/version 绑定到后续执行。执行阶段把每个 tool receipt 关联到步骤，
completion gate 再检查计划中的验证项。保持现有只读工具过滤，先做 artifact/approval，
不要把计划解析器塞进 provider adapter。

**为什么这样优化**

计划从 prompt 文本变成 Agent 可消费的状态，能支持用户审批、恢复、偏离检测和最终验证，
同时保留当前 plan mode 的安全边界。

### P1-C：ContextContributor 串行读取且没有预算/新鲜度合同

**当前现状**

每次 provider 请求前，engine 按注册顺序逐个 `await contributor.pre_turn_context()`，
然后拼接为 turn tail（`engine/mod.rs:1347-1389`）。`ContextContributor` 目前只有返回
字符串和 label，没有 token 预算、优先级、版本、TTL 或并行安全标记
（`context_contributor.rs:20-36`）。

**为什么不合适**

knowledge、memory、skills 等相互独立的读取会叠加等待时间；来源内容过大或每轮都变化时，
模型上下文也会被低价值信息挤占。把所有来源简单改成并发可能破坏有副作用的 contributor，
所以不能直接无条件 `join_all`。

**怎么优化**

扩展 contributor 元数据：`priority`、`max_tokens`、`freshness/ttl`、`provenance` 和
`parallel_safe`。对标记为只读且 parallel-safe 的来源做有界并行；对允许陈旧一轮的来源
采用 last-known-good + 后台刷新；统一在注入前按总预算截断并保留来源标记。首次加载、版本
变化或权限变化仍同步获取。

**为什么这样优化**

这同时改善 Agent 可用知识的信噪比和首 token 前的本地等待，并且把新鲜度/权限语义显式化，
避免为了追求速度注入过期或越权内容。

### P1-D：会话 checkpoint 在每轮同步复制、序列化和写 index

**当前现状**

`save_session()` 每次先 clone 全量 `messages`，再调用 `SessionManager::save` 写 pretty
JSON，随后重新读取并写 `index.json`（`engine/mod.rs:2419-2434`，`session.rs:124-135`、
`191-229`）。它在接受用户消息、工具轮次、自然终止以及多个错误路径被调用。

**为什么不合适**

长 transcript、大 tool output 或高频 tool loop 会让 O(history) clone/JSON/同步 I/O 进入
Agent 热路径，直接延迟下一次 provider 请求；但简单 debounce 或异步丢写会破坏恢复和编辑
回滚语义。

**怎么优化**

引入带单调版本号的 `SessionCheckpoint`：消息和 usage 只在版本变化时序列化，index 更新
合并到同一个受控 blocking worker；关键边界（接受用户消息、工具结果提交、终态、取消）
仍等待 durable ack，非关键重复保存只合并。使用临时文件 + 原子替换，并记录
`clone_ms/serialize_ms/write_ms/index_ms`。

**为什么这样优化**

减少重复全量工作而不改变恢复的持久性边界；先有阶段耗时数据，再决定合并窗口，避免凭感觉
牺牲崩溃恢复。

### P1-E：Goal Judge 已有闭环，但仍是 opt-in 且只看最后文本

**当前现状**

Goal runtime 支持 contract、subgoal、done/continue/wait、进程 barrier 和 parse/transport
breaker；但它只有在配置 goal 后才触发。judge 请求是无工具的一次文本 completion，输入主要是
goal、contract、background process 和 `last_response`（`goal/judge.rs:36-102`、
`goal/runtime.rs:180-297`）。

**为什么不合适**

它能防止 goal session 过早结束，却无法直接读取工作区或验证命令；没有 structured receipt
时，judge 仍然依赖模型在文本中报告证据。普通会话又完全不经过这一层。

**怎么优化**

让 Goal Judge 消费 P0-A 的结构化 evidence summary，而不是只消费 response 文本；将 judge
启用条件绑定到任务风险/完成策略，而非仅绑定 `/goal`。对低风险聊天保持关闭，对代码、文件、
浏览器副作用任务启用小预算验证。judge 仍只做判定，实际读取/执行验证必须通过受控工具或
宿主 verifier 完成。

## 4. 模型缓存：只做支撑优化

当前代码已经明确把缓存稳定部分与动态上下文分开：

- `context.rs:170-176` 把 system prompt 定义为 cache-stable prefix；
- `context.rs:212-218` 将大段稳定内容放在前面；
- `engine/mod.rs:1370-1389` 将日期、goal、contributor 内容放进最后消息的 turn tail；
- `engine/mod.rs:1807-1820` 读取 `cache_read_tokens` / `cache_creation_tokens` 并做诊断。

因此，不应把 memory、RAG、goal 或 plan 重新拼回 system prompt，也不应复制外部 provider
的 marker 方案作为 Flowy Agent 能力优化。可以做的支撑工作是：

1. 把 `pre_provider_ms`、contributor latency、checkpoint latency 和 provider TTFT 分开
   记录，避免把本地准备慢误判为模型慢；
2. 在 cache diagnostic 中同时记录 tool schema/version、plan/goal 状态和 compaction
   边界，便于解释 cache miss；
3. 只对同一 wire shape 的 session 做稳定 cache namespace；绝不能用跨用户的全局固定 key。

这些改动的目标是可观测和降低本地开销，不是把 cache hit 当成 Agent 已完成的证据。

## 5. 实施顺序

1. **先落地 completion policy/evidence receipt**：覆盖普通代码、文件和副作用任务的错误
   “自报完成”。
2. **补充 structured tool failure/recovery**：让失败恢复有方向，并保留现有审批、schema、
   retry group 和 stagnation 上限。
3. **改造 ContextContributor 合同**：预算、优先级、新鲜度和安全并行；先测量再启用后台刷新。
4. **版本化 session checkpoint**：在不改变 durable 边界的前提下移出同步 O(history) 工作。
5. **把 plan mode 升级为 PlanArtifact**：与用户审批、执行步骤和最终 verification 对接。
6. **补 Agent Harness 基准**：至少拆分 context read、prompt assembly、checkpoint、TTFT、
   tool critical path、verification 和完整 turn，而不是只测 provider 请求。

## 6. 不建议的方向

- 不要把外部项目名称当作当前工程名称，也不要继续以外部项目作为主项目名称。
- 不要把 provider/cache marker 当成 Agent 能力缺口的替代解释。
- 不要把 `AgentExecutionEngine` 的跨会话 scheduler、attempt、lease 再复制进
  `AgentEngine`。
- 不要只增加 eval 数量来回应完成质量问题；eval 只能证明 completion gate 是否有效，不能
  代替 gate 本身。
- 不要无条件把所有 contributor 并发化、把所有 checkpoint debounce 化，必须先定义权限、
  新鲜度、崩溃恢复和顺序不变量。

## 7. 外部参考的正确用法

外部项目仅可用于参考某些局部实现（例如异步记忆刷新或 provider split）；外部控制面、
配额和事务状态不属于 Flowy 单轮 Agent Harness。任何外部做法都必须先映射到本文件第 2 节
列出的 Flowy 代码，再决定是否适用；没有本仓库证据的结论不得写成当前项目现状。
