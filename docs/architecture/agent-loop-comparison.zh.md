# Flowy Agent Loop 边界与对照

> 主项目是当前工作区 `C:\code\flowy\allo`（Flowy）。本文先描述 Flowy 自己的 Agent
> Loop，再用外部项目作局部参照；不把 JCode、LoopX 或其他项目当成当前工程，也不是同模型、
> 同工具和同硬件下的跑分报告。

## 结论

Flowy 的直接 Agent Loop 是 `nomi-agent::AgentEngine::execute_turn_inner`。它负责一条
用户请求内的 provider stream、工具调用、工具结果、上下文压缩和终止；宿主在
`nomifun-ai-agent` 负责能力组装；跨会话 execution/step/attempt 则由
`nomifun-agent-execution::AgentExecutionEngine` 负责。

因此，性能与能力优化应按两个边界处理：

- **单轮循环**：优化完成证据、失败恢复、上下文读取、工具关键路径和 checkpoint；
- **长程控制**：优化恢复、租约、重试和执行计划，但不把这些状态复制到单轮循环。

## 1. Flowy 当前循环

```text
接受 user content
  -> 保存 root checkpoint
  -> 预估 token / microcompact / autocompact
  -> 组装当前工具 authority、system prompt、context contributor、turn tail
  -> provider stream
  -> 校验 Done / ToolUse / schema
  -> 工具审批与执行（可安全并发）
  -> artifact/media delivery 与 tool receipt
  -> 失败分类、retry tracker、stagnation guard
  -> 追加结果并 checkpoint
  -> 有 steering 则继续
  -> 有 opt-in goal 则 judge；否则自然 EndTurn
```

这条路径的主要源码证据：

| 阶段 | 代码 |
| --- | --- |
| loop 与安全上限 | `crates/agent/nomi-agent/src/engine/mod.rs:1284-1317` |
| tool authority / plan filter | `engine/mod.rs:1319-1334` |
| contributor 与 turn tail | `engine/mod.rs:1347-1389` |
| provider stream 协议验证 | `engine/mod.rs:1433-1719` |
| tool result / artifact 处理 | `engine/mod.rs:1908-2064` |
| retry / stagnation / steering | `engine/mod.rs:2067-2177` |
| natural EndTurn / optional goal | `engine/mod.rs:1840-1905` |
| compaction | `engine/mod.rs:2273-2360` |

## 2. 与 Agent 能力直接相关的对照

| 维度 | Flowy 当前行为 | 真实缺口或优势 | 优化方向 |
| --- | --- | --- | --- |
| 工具安全 | 每次请求生成 authority；调用前做 schema、审批和 deferred tool 检查 | 安全边界清晰，不应为追求并发绕过 | 只扩充已证明无副作用工具的 `is_concurrency_safe` |
| 工具并发 | 独立且声明安全的调用可批量并行，结果保持输入顺序 | 对 I/O 工具的关键路径有潜在优势，但没有统一基准 | 增加 tool critical-path 指标，按实际 profile 扩容 |
| 长上下文 | microcompact、LLM autocompact、emergency gate 已有 | 能避免 context 直接溢出；压缩会改变历史并造成 cache reset | 测量压缩前后任务成功率，不要只追求 token 数下降 |
| 终止语义 | 普通会话自然 `EndTurn` 即返回；Goal Judge 仅 opt-in | 普通副作用任务可能自报完成 | 引入 `CompletionPolicy` 和结构化 evidence gate |
| 失败恢复 | schema retry + stagnation nudge/abort | 能止损，但运行时失败没有统一 recovery action | 增加结构化 failure class、retryability 和替代策略 |
| 计划能力 | plan mode 过滤为 Info 工具，计划写在响应文本 | 是安全模式，不是可审计计划对象 | `PlanArtifact` + 用户批准 + 步骤 receipt |
| 动态上下文 | host 注册 contributor，每轮串行读取，放在 turn tail | cache prefix 设计正确，但等待和预算合同不完整 | TTL、预算、provenance、受控并发与 last-known-good |
| memory/skills/knowledge | 由宿主按 session wiring 注册；静态 prompt 有缓存，动态来源按 turn 注入 | 能力是条件式可见，不是所有对话自动拥有；过量注入会降低信噪比 | 统一 relevance/size budget，明确来源和新鲜度 |
| steering | 运行中的用户插话在两个边界被消费 | 不需要重启 turn，交互性较好 | 保持 generation ownership，继续测试 race-tail |
| 长程 Agent | 外部 `AgentExecutionEngine` 管 execution、attempt、scheduler、retry | 分层正确，避免单轮循环膨胀 | 只扩展外部执行控制，不下沉到 `AgentEngine` |

## 3. 外部项目只作为局部参考

### JCode

JCode 可作为“动态 memory 后台刷新”或“provider split”类局部实现的参考，但不能据此
断言 Flowy 缺少 memory、cache 或 Agent loop。Flowy 当前已有 `ContextContributor`、稳定
system prompt、turn tail、compaction 和 cache usage 统计；是否采用 JCode 的做法必须先
比较权限、新鲜度、失败降级和 session 语义。

### LoopX

LoopX 的 turn driver、quota、claim、journal 等属于外置控制面，不是模型-工具 Agent Loop。
它们不能用来证明 Flowy 的 `execute_turn_inner` 性能，也不应直接复制进 Flowy 的 engine。
Flowy 的对应长程边界已经是 `nomifun-agent-execution`。

## 4. 优化优先级

1. 对需要实际产物的任务启用 `CompletionPolicy::EvidenceRequired`，解决“模型说完成但没验证”。
2. 让 tool failure 产生结构化 recovery hint，减少重复失败调用。
3. 测量 contributor、prompt assembly、checkpoint、provider TTFT 和 tool critical path，
   再决定并发读取或 checkpoint worker。
4. 将 plan mode 的响应文本升级成可批准、可恢复的 PlanArtifact。
5. 只有在以上指标证明 cache 或 provider 是瓶颈时，才做 provider-aware cache 优化。

## 5. 不应作出的结论

- 没有同一 workload、模型、网络和工具集，不能声称 Flowy 整体比 JCode 或其他项目更快。
- 有停滞 guard 不等于 Agent 已经会恢复；它首先是止损机制。
- 有 Goal Judge 不等于普通会话获得了完成验证；当前 goal 明确是 opt-in。
- 有 prompt cache diagnostics 不等于模型缓存命中率已经被优化；需要真实 usage 和本地阶段
  时延分解。
