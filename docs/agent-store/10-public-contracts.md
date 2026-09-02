# Agent Store 公共契约索引

> 状态：公共契约冻结（Phase 0）；实现待验证
> 日期：2026-08-26
> 用途：跨文档共享的状态、事件、planning、认证、错误和幂等契约
> 原则：只定义公共语义和序列化名称，不描述 allo 内部实现

## 1. 唯一事实来源

### Agent 术语边界（规范）

Agent Store 的 Agent 是产品层定义，在 allo 中通过 Preset/`ResolvedPresetSnapshot` 承载；nomifun 的 Agent 才是 Claude Code、Codex 这类实际 Runtime Agent/Driver。公共 `AgentId` 指 Agent Store AgentDefinition/Preset 的 ID，不表示 Runtime Agent 实例身份。

| 主题 | 文档 |
|---|---|
| 架构决策 | `00-architecture-decision.md` |
| 对象、状态、事件 | `01-domain-model.md` 与本文 |
| 导入规则 | `02-codebuddy-workbuddy-import-spec.md` |
| 兼容性 | `03-codebuddy-compatibility-matrix.md` 与本文 |
| allo 映射、恢复 | `04-allo-runtime-adapter.md` |
| Protocol | `05-allo-app-server-protocol.md` 与本文 |
| 凭据、安全 | `06-connector-oauth-security.md` |
| SDK | `07-typescript-sdk.md` |
| Web/Flowy | `08-flowy-web-integration.md` |
| 发布门禁 | `09-release-readiness.md` |
| 排期 | `agent-store-v1-roadmap.md` |
| 测试 | `agent-store-v1-test-cases.md` |

冲突处理：跨文档枚举和字段以本文为准；对象语义以 `01` 为准；Runtime 细节以 `04` 为准；安全规则以 `06` 为准。

## 2. 执行状态

```text
RunStatus:
  queued | starting | running | paused | completed | failed | cancelled | recovery_required
StepStatus:
  pending | ready | in_progress | completed | failed | cancelled
AttemptStatus:
  created | running | completed | failed | cancelled | stale
DefinitionStatus:
  draft | imported | validated | enabled | disabled | superseded
```

V1 执行树：

```text
PlanRevision → Step → Attempt
```

`Task`、Shared Task List、Mailbox 和成员自主认领不是 V1 独立公共执行对象。

## 3. 规范事件

```text
run.started | run.paused | run.completed | run.failed | run.cancelled
plan.created | plan.revised
step.ready | step.started | step.completed | step.failed
attempt.started | attempt.completed | attempt.failed
approval.required | artifact.created
```

事件信封：

```text
event_id / stream_id / resource_type / resource_id
 type / timestamp / data
```

Event Log 是 allo 引擎内部的事实来源；V1 公共契约只承诺状态与结果的持久化查询。事件通知尽力而为，不承诺 cursor 重放与断线追平；终态不能被迟到写入覆盖。V1 不承诺服务端重启后续跑原 Attempt；未完成 Run 必须明确标记为 `recovery_required`/`failed`。

## 4. Team Planning

V1 Team 采用模式 A：TeamRun 由服务端直接构造 Planning Context，并调用内部 `Planner/LlmPlanProducer` 生成结构化 planned DAG。`lead_agent_id` 表示规划角色；它不要求对应一个独立的用户可见 Conversation，也不要求通过模型可见的 `nomi_delegate` 工具启动 Team。

Planning Context 由以下部分组成：

```text
Leader Preset 的规划指令
+ Team 目标与 planner_policy
+ 脱敏成员能力摘要（name/role/description/model/strengths）
+ routing_constraints、workflow_limits 和有效策略
```

成员的完整 persona、Skill、Connector、Tool Policy 和凭据引用不进入共享 Planning Context；它们在 TeamRun 创建时分别冻结到各成员的 Participant/ResolvedPresetSnapshot 中。Planning Context 是本次运行的派生输入，可记录 `planning_context_digest` 用于审计和复现，但不是新的产品定义对象。

公共 JSON：

```json
{
  "mode": "planned",
  "adaptation_policy": "fixed | adaptive",
  "plan_gate": "automatic | approval",
  "max_parallel": 4
}
```

TypeScript 映射：

```ts
interface PlanningOptions {
  mode?: "planned";
  adaptationPolicy?: "fixed" | "adaptive";
  planGate?: "automatic" | "approval";
  maxParallel?: number;
}
```

V1 只支持固定成员、Planning Context 驱动的 planned DAG、局部并行、有限 retry 和 replan。服务端必须在 Plan 物化前校验每个 Step 的成员路由、依赖、工具策略和并发限制；Prompt 不能替代这些校验。普通可信会话仍可使用 `nomi_delegate`，但它不是 TeamRun 的必要依赖。

## 5. 兼容性维度

```text
SemanticStatus:
  compatible | compatible-with-adapter | manual-review | unsupported | pending-legal-review
RuntimeStatus:
  not-verified | adapter-verified | runtime-verified | release-eligible
DistributionStatus:
  local-only | installable | public
```

`unsupported-auth`、`ignored-by-source-runtime` 是原因码，不是一级状态。`pending-legal-review` 覆盖分发状态。

## 6. 认证、幂等与 Approval

V1 默认本地认证：stdio 或 localhost WebSocket 由主进程建立 `LocalPrincipal` 和 `AuthContext`；Renderer 不自行声明身份。

有副作用的 Command 关联：

```text
command_id / idempotency_key / expected_version（可选） / AuthContext.principal_id
```

幂等作用域为：

```text
principal_id + client_id + method
```

相同 key 和指纹返回原 receipt；指纹不同返回 `idempotency_conflict`。

Approval 同时表示：

```text
approval.required = 事实事件
approval/request   = Server Request
approval/respond   = 客户端 Command
```

三者共享 `request_id`、`approval_id`、`run_id`、`step_id`、`attempt_id`。请求还必须绑定 `tool_call_id`、`argument_digest` 和 `expires_at`。重复响应幂等，过期响应返回 `approval_expired`。

## 7. 公共错误码

```text
unauthenticated | invalid_issuer | invalid_audience | insufficient_scope
policy_denied | protocol_version_unsupported | idempotency_conflict
approval_expired | approval_already_resolved | unsupported_operation
recovery_required | credential_unavailable | reauthorization_required
import_source_not_found | import_blocked | import_failed
internal_error
```

`import_source_not_found`：`import/run` 的本地来源目录不存在或不可读（HTTP 404，对应 `NotFound`）；`import_blocked` / `import_failed`：快照因路径安全、清单身份缺失或 digest 冲突而阻断，或导入器内部失败。阻断原因以结构化 `ImportResult.errors` 返回，错误文本只含清单相对值与原因码，**不得包含绝对来源路径或凭据**（02 §9）。

错误响应不得包含真实凭据、内部路径、内部 ID 或未脱敏的上游响应。

## 8. 变更规则

1. 公共枚举和字段先修改本文；
2. 同步 Protocol Schema、SDK 类型和测试主表；
3. 其他文档只引用契约，不复制另一套枚举；
4. 变更必须记录兼容影响和回归测试编号；
5. 未经 Runtime/Protocol 测试验证的能力不得标记 `release-eligible`。
