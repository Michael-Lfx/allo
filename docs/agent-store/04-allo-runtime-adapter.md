# allo Runtime Adapter 规格

> 状态：架构冻结（Phase 0）；Runtime Adapter 待实现验证；发布阻断
> 日期：2026-08-26
> 前置：`00-architecture-decision.md`、`01-domain-model.md`
> 目标：定义 Agent Store 领域模型如何映射到 allo Runtime；allo 是唯一 Runtime，Adapter 只隔离内部实现

## 1. 目标与非目标

### 1.1 目标

- 将 Agent Store 的 Agent/Team/Skill/Connector/Run 模型映射到 allo；
- 使用 allo 现有 Agent Execution、ExecutionTemplate、Planner、Participant Resolver、事件基础设施；
- V1 实现单 Agent 与固定成员 AgentTeam；
- V1 Team 支持由 Leader Planning Context 驱动的 planned DAG、局部并行、retry、replan、事件和 Artifact；
- 不把 allo 内部 ID、数据库模型、UI 事件形态暴露给 App Server。

### 1.2 非目标

- 不支持第二个 Runtime；
- 不把 CodeBuddy/WorkBuddy 来源格式直接交给 allo；
- 不实现完整 CodeBuddy Team 的 Mailbox、成员自主认领、成员直连消息和嵌套 Team；
- 不让模型直接决定权限、成员路由、状态迁移或审批结果。

## 2. 映射总览

```text
PluginSnapshot
    ↓ Importer
AgentDefinition / AgentTeamDefinition / SkillDefinition / ConnectorDefinition
    ↓ Runtime Adapter
Allo Preset / ResolvedPresetSnapshot / Skill / MCP Config / ExecutionTemplate
    ↓
Conversation / Execution / Participant / Plan / Step / Attempt
    ↓ Event Normalizer
Agent Store Run / Task / Event / Artifact
```

| Agent Store | allo 目标 | 说明 |
|---|---|---|
| AgentDefinition | Preset/ResolvedPresetSnapshot + ExecutionParticipant | Agent Store AgentDefinition 先解析为现有 Preset 快照；实际 Runtime Agent/Driver 单独选择和执行 |
| SkillDefinition | allo Skill/Skill reference | 运行前校验路径与版本 |
| ConnectorDefinition | MCP Config/Connector runtime | 工具名必须经命名空间和策略过滤 |
| AgentTeamDefinition | AgentExecutionTemplate | 保存固定成员、角色、模型/技能/工具快照 |
| TeamRun | Execution + Planning Context | V1 由服务端 Planner 生成 planned DAG；不创建独立 Leader Conversation |
| Task/Step | Execution Step | 用 dependency/participant_index 映射 |
| Attempt | Attempt/Conversation execution | 公开 opaque attempt_id |
| Event | Allo event/sequence → canonical Event | 不直接转发 provider/UI 事件 |
| Artifact | Artifact reference | 文件实际内容保留在受控 workspace/artifact store |

## 3. Agent Run

### 3.1 创建前校验

运行时必须依次校验：

1. AgentDefinition 存在且版本可用；
2. AgentDefinition 能解析为合法的 Preset/ResolvedPresetSnapshot，且内容 digest 与 Catalog 记录一致；
3. 所有 Skill refs 存在、版本匹配且路径在快照内；
4. Connector refs 已安装，工具策略可解析；
5. CredentialBinding 存在且权限范围满足；
6. workspace 已登记、路径允许、不可越界；
7. 调用方策略与 Agent/Skill/Connector 策略交集非空；
8. 模型和 Runtime 能力满足 Definition 要求。

校验失败必须返回结构化错误，不创建可运行 Run。

### 3.2 Persona 组装

不要将导入 Markdown 直接拼进未定义的全局 system prompt。运行时按显式区段组装：

```text
[Agent Identity]
[Role / Persona]
[Bound Skills]
[Allowed Connector Tools]
[Workspace Policy]
[Current Task]
[Output Contract]
```

工具、文件、网络和凭据权限由 Runtime Policy 计算，不由 Persona 文本决定。

### 3.3 生命周期

```text
创建公共 run_id
    → 记录 execution snapshot
    → 创建 allo Conversation/Execution
    → 持久化 run.started
    → 订阅并规范化事件
    → 保存 Artifact
    → 写入 completed/failed/cancelled
```

App Server 只返回 Agent Store 公共 ID，不返回 allo 内部 session/execution ID。

## 4. Team Run（V1）

### 4.1 固定成员物化

Importer 只生成 `AgentTeamDefinition` 和标准化成员引用，不直接创建 allo 的 `AgentExecutionTemplate`。`AgentExecutionTemplate` 是 Runtime Adapter 在 TeamRun 创建时生成的执行级快照，避免 Importer 依赖 allo 内部对象。

```text
AgentTeamDefinition vN
    ↓
解析 lead_agent_id 与 member_agent_ids
    ↓
分别冻结 Leader 和每个成员的 Preset/ResolvedPresetSnapshot、Skill/Connector/Tool Policy 配置快照
    ↓
创建 AgentExecutionTemplate 与固定 Participant 池
    ↓
构造 Planning Context（Leader 规划指令 + 脱敏成员能力摘要 + Team 策略）
    ↓
创建 TeamRun 并调用内部 Planner
```

成员列表来自 TeamDefinition，不能由模型在运行中任意增删。成员角色约束必须在 Runtime 代码中校验。

### 4.2 Leader planned 流程（模式 A）

这里的 Leader 是 `lead_agent_id` 指向的规划角色，不要求创建独立的用户可见 Conversation。TeamRun 创建时，Runtime Adapter 将以下内容组成一次性的 Planning Context，交给内部 `Planner/LlmPlanProducer`：

```text
Leader Preset 的规划指令
    + Team 目标与 planner_policy
    + 脱敏的成员能力摘要（name/role/description/model/strengths）
    + routing_constraints、workflow_limits 和有效策略
```

成员的完整 persona、Skill、Connector 和 Tool Policy 不进入共享 Planning Context，而是保留在各自 Participant 的不可变 Snapshot 中。

```text
TeamRun 创建
    ↓
解析 TeamDefinition 并物化固定 Participant 池
    ↓
构造 Planning Context
    ↓
Planner 生成结构化 Plan/DAG
    ↓
校验依赖、成员路由、并发限制和工具策略
    ↓
物化 Step
    ↓
调度 ready Step
    ↓
收集结果并更新状态
    ↓
必要时 retry 或 replan
    ↓
汇总 TeamRun 结果
```

V1 允许：

- 顺序依赖；
- 依赖满足后的 ready Step；
- 独立 ready Step 的局部并行；
- 失败 Step 的有限 retry；
- 基于结果、失败或用户指令的 replan；
- 汇总、验证和 QA→Engineer→QA 反馈回路。

V1 不允许：

- 动态创建 Team 外成员；
- 嵌套 Team；
- 模型绕过 routing_constraints 指定成员；
- 直接把任意 `role` 字符串当权限边界；
- 顶层用 `strategy=parallel` 代替 Team planned 流程。

### 4.3 Planner 参数映射

V1 Team 的 `planned` 是 Runtime Adapter 对内部 Planner 的执行策略，不要求通过模型可见的 `nomi_delegate` 工具触发。普通可信会话仍可使用 `nomi_delegate`；它不是 TeamRun 的必要依赖。

Runtime Adapter 只允许向内部 Planner 传递经校验的规划参数；这些参数不是公共协议，也不是模型可见工具的 delegation 参数：

```json
{
  "mode": "planned",
  "goal": "<run goal>",
  "plan_gate": "automatic",
  "adaptation_policy": "adaptive",
  "max_parallel": 4
}
```

`work_dir`、`model_pool` 按 Team/Run Policy 生成并限制范围；不得由外部客户端直接覆盖继承的安全约束。

### 4.4 Step 调度不变量

- Step 依赖必须指向同一 TeamRun 内已存在的 Step；
- 只有所有依赖成功或被明确允许跳过时，Step 才能 ready；
- `participant_index` 必须落在固定 Participant 池内；
- `max_parallel` 不得超过 Team、调用方和 Runtime 上限的最小值；
- 已取消、失败终态的 Step 不得被迟到事件覆盖；
- retry 必须生成新的 Attempt；旧 Attempt 的迟到写入必须被拒绝；
- replan 生成新 Plan Revision，不覆盖历史计划和事件。

### 4.5 Runtime Readiness Gate

Agent Store Agent/Team 只有通过以下阶段才能进入对应的运行状态：

```text
model-defined
    → adapter-defined
    → runtime-verified
    → release-eligible
```

`runtime-verified` 至少要求：

1. AgentDefinition 能解析为不可变 Preset/ResolvedPresetSnapshot；
2. Preset Snapshot 能绑定到 allo ExecutionParticipant，并由 Runtime Agent/Driver 创建 Conversation/Attempt；
3. 单 Agent Run 能产生 started、completed/failed 事件和结果；
4. 事件、结果和内部错误能被 Adapter 规范化；
5. 测试证据记录 allo 版本、Definition digest、请求和事件序列。

Team 不得在单 Agent Gate 通过前进入 `runtime-verified`。

## 5. 事件与 Artifact 规范化

### 5.1 统一事件

Adapter 将 allo 事件映射为：

```text
run.started
run.paused
plan.created
plan.revised
step.ready
step.started
step.completed
step.failed
attempt.started
attempt.completed
attempt.failed
tool.call.started
tool.call.completed
approval.required
artifact.created
run.completed
run.failed
run.cancelled
```

事件必须带：

```text
event_id
stream_id
resource_type
resource_id
type
timestamp
data
```

### 5.2 Artifact

Artifact 只公开：

```text
artifact_id
run_id
name
media_type
size
sha256
workspace_relative_path
created_at
```

真实文件存储在受控 workspace/artifact store；禁止通过事件、日志或公共 API 返回凭据。

## 6. 错误与恢复

### 6.1 事实来源与提交顺序

```text
Command
    → 鉴权/策略校验
    → 写入幂等 Intent
    → allo 执行
    → 持久化规范 Event
    → Projector 更新 Run/Step/Attempt 投影
    → 对外返回/推送
```

Event Log 是 allo 引擎内部的事实来源；Run、Step、Attempt 和当前 Plan Revision 的持久化状态是 V1 的读取模型。事件序号（sequence）由引擎在持久化 Event 时分配，仅在内部使用；公共 cursor 列为 V2，不接受客户端提供的 cursor。

状态写入安全由 allo 引擎内部既有的版本/CAS、lease/fencing 与 receipt 机制保障；这些属于引擎内部实现，不是 V1 App Server 公共契约。Agent Store 层的投影 CAS 与公共 Attempt fencing token 列为 V2。终态不得被迟到写入覆盖的要求不变。

### 6.2 崩溃恢复

V1 复用 allo 既有的启动恢复和内部安全保护：恢复会根据持久化状态处理 queued/running Attempt，采用已确认的 receipt，或将无法证明安全的执行阻塞为 `recovery_required`。App Server 不对外承诺服务端一定续跑原 Attempt，但不删除、不旁路 allo 的 Intent、版本/CAS、lease/fencing、receipt 和恢复扫描机制。需要重跑时由用户发起新的 Run/Attempt，并继续遵守 Connector 幂等和审批要求。

Adapter 错误分为：

```text
invalid_definition
policy_denied
credential_unavailable
workspace_denied
runtime_unavailable
plan_invalid
step_dependency_invalid
participant_unavailable
attempt_stale
cancelled
internal_error
```

V1 重启要求：

- 保留已持久化事件和终态；
- 未完成 Run 必须明确标记 `recovery_required`/`failed`；
- 不能把“进程退出”伪装成 completed；
- App Server 对外只承诺状态如实可查询；allo 内部可按既有安全恢复规则继续处理，无法证明安全时必须保持 `recovery_required`，不覆盖原结果。

服务端恢复语义是否作为 App Server 公共能力开放、以及跨 Run 的恢复操作 API，列为 V2；allo 内部既有安全恢复机制属于 V1 Runtime 实现并继续保留。

## 7. V1 验收用例

- `TC-RA-001`：AgentDefinition 成功解析为 Preset，并由 Runtime Agent/Driver 完成单 Agent Run；
- `TC-RA-002`：Skill 和 Connector Policy 在运行前正确加载；
- `TC-RA-003`：software-company 生成固定 5 人 Participant 池；
- `TC-RA-004`：Planning Context 驱动 Planner 生成顺序 DAG 并绑定正确 Participant；
- `TC-RA-005`：两个独立 Step 局部并行，依赖 Step 等待完成；
- `TC-RA-006`：Step 失败后生成新 Attempt 并 retry；
- `TC-RA-007`：QA 失败后 replan 产生修复与回归 Step；
- `TC-RA-008`：旧 Attempt 迟到事件不能覆盖新 Attempt；
- `TC-RA-009`：Runtime 重启后保留事件和终态，未完成 Run 明确标记失败或需恢复；
- `TC-RA-010`：公共响应不泄露 allo 内部 ID 和凭据。

## 8. 实现顺序

1. 定义 Adapter 内部接口和 ID 映射；
2. 完成单 Agent 创建、事件和结果；
3. 完成 AgentExecutionTemplate 固定成员物化和 Planning Context 构造；
4. 接入内部 planned 计划生成和 Step 物化；
5. 完成 ready Step 调度、并发限制和 Attempt；
6. 完成 retry/replan；
7. 完成 Artifact 与启动状态检查，将未完成 Run 标记为 `recovery_required`/`failed`（引擎内部 sequence 继续存在，不对对外承诺 cursor）；
8. 运行 software-company V1 验收用例。
