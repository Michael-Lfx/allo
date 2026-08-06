# Flowy Agent Harness 优化提案

> 本文只针对当前项目 `C:\code\flowy\allo`。权威的源码证据和完整评审见
> [Agent Harness 能力、性能与优化审查](architecture/agent-harness-capability-analysis.zh.md)。
> 本文不把任何外部项目当作 Flowy 的实现边界。

## 判断标准

优化必须同时说明四件事：

1. 当前代码已经做了什么；
2. 哪个行为或热路径不合适；
3. 改动落在哪个 Flowy 模块；
4. 为什么改动能提升 Agent 能力或降低本地延迟，并且保留哪些不变量。

仅增加测试、eval 或 provider 参数不算能力优化；测试只用于证明优化是否有效。

## 当前已具备的能力

- `nomi-agent::AgentEngine` 已处理 provider stream、工具 authority、schema、审批、工具
  执行、artifact delivery、上下文压缩、stagnation guard、steering 和 session checkpoint。
- plan mode 已限制为 Info 工具，退出时恢复原 allow-list。
- Goal runtime 已支持 contract、subgoal、done/continue/wait、进程 barrier 和 judge
  失败 breaker，但 goal 是 opt-in。
- `ContextContributor` 已把动态 memory/knowledge/skills 放到 turn tail，稳定 system
  prompt 可复用；host wiring 决定具体会话能看到哪些来源。
- `nomifun-agent-execution::AgentExecutionEngine` 已在 engine 外部承担跨会话 execution、
  step、attempt、scheduler 和 retry。

## P0：把“完成”从模型自报变成可验证状态

### 当前问题

普通 engine 默认没有 goal。自然 `EndTurn` 且没有 tool call 时，
`execute_turn_inner` 会直接返回成功形状的 `AgentResult`。系统提示中的“运行测试/构建后
再报告”只是模型规则，不是运行时 gate。

### 优化方案

在宿主任务配置增加 `CompletionPolicy`：普通聊天使用 `Conversation`；文件、代码和浏览器
副作用任务使用 `EvidenceRequired`；长程目标使用 `GoalJudged`。Evidence gate 消费工具
receipt、artifact locator、命令退出码和 verifier 输出，而不是只读取最后一段回答。

### 为什么这样优化

模型文本可以声称完成，结构化工具记录才能证明状态已经发生。将 gate 放在自然终止边界，
不会改变 provider 或普通聊天延迟；验证失败时追加 continuation，预算耗尽时明确返回
`Incomplete/Blocked`。

## P1：让工具失败产生恢复动作

### 当前问题

现有 `ToolRetryTracker` 主要追踪明确的 schema failure；运行时失败依赖相同 outcome signature、
all-failed counter 和 stagnation nudge/abort。它可以止损，但不能统一表达超时、权限拒绝、
资源不存在或暂时性外部错误。

### 优化方案

引入内部 `ToolFailureContext`：`kind`、`retryable`、`attempt_no`、`retry_group_id`、
`recommended_next_action` 和 artifact 关联。下一轮向模型提供结构化 recovery hint；不可重试
错误不再重复，可重试错误继续受上限约束。

### 为什么这样优化

失败恢复从“模型猜错误文本”变成“按类别采取动作”，可减少重复工具调用，同时不放宽现有
authority、schema 和审批边界。

## P1：控制动态上下文的等待和信噪比

### 当前问题

每轮 provider 请求前，contributors 按注册顺序串行 await；接口只有字符串和 label，没有
预算、优先级、新鲜度或 provenance。来源增多后，首 token 前等待和无关上下文都会增长。

### 优化方案

扩展 contributor 元数据，按总 token 预算排序和截断；只对只读且显式标记
`parallel_safe` 的来源做有界并行；允许陈旧一轮的来源使用 last-known-good 后台刷新，
权限/版本变化时强制同步刷新。

### 为什么这样优化

同时减少本地等待和上下文噪音，但不会把所有 contributor 无条件并发化，也不会让过期或越权
内容进入 prompt。

## P1：降低 session checkpoint 对循环的阻塞

### 当前问题

`save_session()` 每轮 clone 全量消息，pretty-serialize session，同步写 session 文件，再
同步读写 `index.json`。长 transcript 和大工具输出会把 O(history) 工作放在下一次 provider
请求前。

### 优化方案

使用带单调版本号的 checkpoint worker：关键边界等待 durable ack，重复版本合并；index 更新
跟随同一版本提交；临时文件原子替换；记录 clone/serialize/write/index 分段耗时。

### 为什么这样优化

保留恢复、取消和 editable-turn 回滚语义，去掉重复全量序列化和同步 I/O；没有测量前不做
无界 debounce 或丢写。

## P1：把 plan mode 变成可审计计划

### 当前问题

plan mode 当前主要是 Info 工具过滤和提示词约束，计划写在回答文本中，`ExitPlanMode` 的
`plan_content` 仍为 `None`。宿主不能可靠地审批、恢复或检查实际执行是否偏离计划。

### 优化方案

新增 `PlanArtifact`（目标、步骤、范围、验证、风险、版本），退出 plan mode 时提交；宿主
批准后绑定 artifact version，工具 receipt 关联步骤，最终 completion gate 检查验证项。

### 为什么这样优化

计划由 prompt 文本升级为 Agent 状态，支持用户审批、恢复和偏离检测，同时保留现有只读
工具安全边界。

## 模型缓存的边界

当前 Flowy 已把稳定 system prompt 与动态 turn tail 分开，并记录 cache read/create usage。
缓存优化只承担两个任务：

- 分离 contributor、checkpoint 和 provider TTFT，避免把本地准备耗时误报为模型延迟；
- 在诊断中记录 tool schema/version、plan/goal 状态和 compaction 边界，解释 cache miss。

不应把 Agent 能力问题改写成 provider/cache 问题，也不应把动态 memory/RAG 重新放回稳定
system prefix。

## 实施顺序

1. `CompletionPolicy` + evidence receipt；
2. `ToolFailureContext` + recovery hint；
3. contributor budget/TTL/provenance/受控并行；
4. 版本化 session checkpoint；
5. `PlanArtifact` 与审批/验证闭环；
6. 最后再依据阶段耗时和真实 cache usage 做 provider-aware 微调。
