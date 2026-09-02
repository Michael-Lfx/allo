# Agent Store 领域模型

> 状态：架构冻结（Phase 0）；领域模型待 Runtime 验证；发布阻断
> 日期：2026-08-26
> 前置：`00-architecture-decision.md`
> 口径：对象、字段与边界；区分「源码/文档已证实」「设计决策」「待验证」

## 1. 目的

统一 Agent Store 的产品对象与边界，供导入、Runtime、App Server、SDK、Web 共同引用。任何层不得自行另立一套语义。

## 2. 核心对象总览

### 三层对象关系（规范）

Agent Store 的 `Agent`/`AgentDefinition` 是产品层可复用定义；allo 不把它当作 Runtime Agent，而是通过现有 `Preset` 解析为 `ResolvedPresetSnapshot`，写入 `ExecutionParticipant.preset_*`。`nomifun` 中的 Agent 才表示 Claude Code、Codex 等实际 Runtime Agent/Driver，负责创建或持有 Conversation/Attempt 并执行模型和工具调用。

```text
Agent Store Agent/AgentDefinition
    → allo Preset/ResolvedPresetSnapshot
    → ExecutionParticipant.preset_id/preset_revision/preset_snapshot
    → nomifun Runtime Agent/Driver
    → AgentExecution
```

| 对象 | 层次 | 说明 |
|---|---|---|
| PluginSnapshot | 来源/分发 | 一次导入生成的不可变快照，含来源、版本、digest、组件与兼容性 |
| AgentDefinition | 定义 | Agent Store 中可复用的专家配置定义；在 allo 中主要通过 Preset 机制承载 |
| AgentTeamDefinition | 定义 | 固定成员名册 + 协作规则 |
| SkillDefinition | 定义 | 原子能力，被 Agent 调用，不独立对话 |
| ConnectorDefinition | 定义 | 与外部系统交互的受管能力 |
| CommandDefinition | 定义 | 用户可调用的提示/命令 |
| LifecycleHookDefinition | 定义 | 生命周期事件钩子（策略门控） |
| LspDefinition | 定义 | 语言服务器能力（V1 元数据级） |
| CredentialSchema | 定义 | 连接器所需凭据的声明 |
| CredentialBinding | 运行时 | 具体主体对具体连接器的凭据绑定（只存引用） |
| Run / TeamRun | 运行时 | 一次执行的持久化生命周期 |
| Planning Context | 运行时派生输入 | TeamRun 本次规划使用的 Leader 指令、脱敏成员能力摘要与 Team 策略；不是产品定义或独立公共 Run |
| Task | 运行时 | 团队/执行内的任务单元 |
| Attempt | 运行时 | 一次任务的执行尝试 |
| Plan / DAG / Step | 运行时 | Planner 根据 Planning Context 生成的执行计划 |
| Event | 运行时 | 持久化规范事件（内部 sequence；公共 cursor 为 V2） |
| Artifact | 运行时 | 执行产物 |
| Approval | 运行时 | 需要用户决策的审批点 |
| Provenance | 公共 | 来源、导入时间、digest、兼容性状态 |

## 3. 边界定义（必须分清）

| 容易混淆的对 | 正确边界 |
|---|---|
| Agent Store Agent vs Preset | AgentDefinition 是 Agent Store 产品层的可复用专家定义；在 allo 中通过 Preset/ResolvedPresetSnapshot 承载。Preset 是配置快照，不是实际 Runtime Agent |
| Runtime Agent vs Agent Store Agent | nomifun Agent 指 Claude Code、Codex 等实际 Runtime Agent/Driver，负责持有 Conversation/Attempt 并执行模型和工具调用；不能与 Agent Store AgentDefinition 混用 |
| Agent vs sub-agent | sub-agent 是一次执行内生成的辅助执行单元（对应 allo 执行级 Step/Attempt），不是可复用 AgentDefinition |
| AgentTeamDefinition vs AgentExecutionTemplate | 前者是产品级可版本化团队定义；后者是 allo 当前执行级固定 Participant 快照（源码已证实），用于 V1 Team Runtime；后者不等同完整 Team Runtime |
| Skill vs Connector | Skill 是「怎么做」（指令/流程）；Connector 是「访问什么」（外部系统/凭据/工具）。Skill 可引用 Connector |
| PluginSnapshot vs Runtime Definition | 快照是来源的不可变镜像；运行时定义是经过映射后的可执行定义 |
| TeamDefinition vs TeamRun | 定义可复用；TeamRun 是一次执行实例（含固定成员 Participant、计划、Step、Attempt 和事件） |
| 来源 slug vs 业务 ID | 来源 slug（如 `software-team-lead`）只作来源标识，业务 ID 采用 `wb-<plugin>-<slug>` 等映射规则，allo 内部 ID 一律 opaque |

## 4. AgentDefinition

```text
AgentDefinition
├── id / version
├── name / display_name
├── description
├── persona                          # 角色指令（与全局 system prompt 分段组装）
├── model_profile                    # 默认模型、候选模型（可空）
├── skill_refs[]                     # 绑定 SkillDefinition
├── connector_refs[]                 # 可访问连接器（经策略交集生效）
├── tool_policy                      # allow / deny / 访问模式
├── turn_budget                      # max_turns 等
├── memory_policy                    # 记忆策略（可空）
├── isolation_policy                 # 工作区/隔离（如 worktree，V1 标记 manual-review）
├── quick_prompts[]                  # 示例启动模板（可空）
├── source                           # Provenance
└── compatibility_status
```

规则：

- persona 不写入未定义的全局 systemPrompt；运行时按显式分段组装；
- `skill_refs`/`connector_refs` 只允许引用已导入且状态有效的定义；
- CodeBuddy `agents/*.md` 的 frontmatter 字段（model/effort/maxTurns/tools/disallowedTools/skills/memory/background/isolation）必须保留为结构化字段，不能只保留正文。

## 5. AgentTeamDefinition

```text
AgentTeamDefinition
├── id / version
├── name / description
├── lead_agent_id
├── member_agent_ids[]
├── shared_skill_refs[]
├── planner_policy          # 默认 planned、adaptation（fixed/adaptive）、plan_gate（automatic/require_approval）
├── routing_constraints      # 角色→参与者强制路由（如 PRD 必须 PM）
├── coordination_policy      # V1 固定：leader_planned（模式 A：规划上下文驱动）；mailbox 等为后续字段
├── workflow_limits          # max_parallel、max_participants、递归深度
├── allowed_connectors[]
├── source
└── compatibility_status
```

运行形态（V1 最小 Team Runtime）：

```text
AgentTeamDefinition
    → Leader 与成员分别解析 Preset Snapshot
    → 固定 Participant 池（AgentExecutionTemplate 承载）
    → Planning Context（Leader 规划指令 + 成员能力摘要 + Team 策略）
    → Planner 动态 DAG（顺序/局部并行/汇总/验证/反馈）
    → 事件、重试、replan
```

V1 不要求完整 Mailbox、成员自主认领、成员直连消息、长期成员会话和嵌套 Team。

Planning Context 是 TeamRun 创建时派生的内部规划输入：

```text
Leader 的规划指令
+ Team 目标与规划策略
+ 脱敏的成员能力摘要
+ routing_constraints、workflow_limits 和有效权限策略
```

它不包含成员完整 Prompt、真实凭据或未授权工具细节；公共接口最多返回其 digest。每个成员的完整 Prompt 和能力配置仍只绑定到该成员自己的 Participant/Attempt Snapshot。

## 6. SkillDefinition

```text
SkillDefinition
├── id / version
├── name / description
├── mode                 # client-instructions（纯知识）| store-agent（受控执行）| store-workflow（确定性/高风险流程）
├── instructions_ref     # SKILL.md（正文 + frontmatter + $ARGUMENTS 支持）
├── references[] / scripts[] / templates[] / assets[]
├── required_connectors[]
├── invocation_policy    # @调用 / /调用 / 模型自动调用
├── source
└── compatibility_status
```

规则：

- 默认 store 执行；纯知识型可暴露过滤后的说明，不暴露原始内部 Markdown、凭据或内部路由规则；
- 脚本/可执行内容在安全模型落地前不自动执行，安装期只复制与解析；
- 发送、发布、删除、部署等副作用必须经 Connector 策略与审批，不能只靠 Prompt 控制。

## 7. ConnectorDefinition

```text
ConnectorDefinition
├── id / version
├── kind        # remote-mcp | stdio-mcp | cli | http-api | composite
├── transport   # 端点/命令 + 参数
├── tool_filter      # 暴露/命名空间规则（connector__<name>__<tool>）
├── auth_mode   # none | apikey | env | oauth | cli-login
├── credential_schema（CredentialSchema 引用）
├── scope       # 允许域、命令白名单（CLI）
├── skill_refs[]
├── source
└── compatibility_status
```

规则：

- Connector 不等于 MCP URL；CLI/HTTP/复合连接器走受控适配器，不暴露任意 shell；
- 标准、只读 MCP 可用命名空间代理工具；高副作用操作包装为 Skill/Agent/Workflow + 审批；
- 凭据绑定按 principal + connector + issuer + resource + scopes 隔离，不按 server_url 单键。

### CredentialSchema

```text
CredentialSchema
├── id
├── fields[]      # key、type（apikey/oauth/token/env/secret）、required、sensitive
├── oauth_meta    # discovery、scopes、PKCE、loopback（如适用）
└── source
```

敏感字段一律只存引用/密文于安全存储；日志、Markdown、前端状态、公共 API 响应不得出现真实值（文档中统一 `[REDACTED]`）。

## 8. 运行时对象

### Run / TeamRun

```text
Run
├── id（opaque 公共 ID）
├── kind        # agent-run | team-run
├── target_ref  # AgentDefinition / AgentTeamDefinition 引用
├── workspace_id
├── status      # queued → starting → running → completed | failed | cancelled | paused
├── events[]    # 指向规范事件流
├── artifacts[]
├── approvals[]
└── created_at / updated_at
```

TeamRun 追加：

```text
TeamRun
├── lead_agent_ref                 # 规划角色的 AgentDefinition 引用
├── planning_context_digest        # 本次规划上下文摘要的 digest
├── member_participant_refs[]      # 固定成员的运行时引用
├── plan_revisions[]
├── current_plan_revision_id
├── steps[]       # V1 唯一正式的 DAG 执行节点
├── attempts[]    # Step 的执行尝试
└── mailbox       # 后续范围
```

### Step / Attempt（V1 正式执行模型）

```text
Step
├── id
├── plan_revision_id
├── title / prompt
├── participant_ref（固定成员）
├── status   # pending → ready → in_progress → completed | failed | cancelled
├── depends_on[]
└── retry_policy

Attempt
├── id（opaque）
├── step_ref
├── status
└── result_ref
```

规则：

- 每次 Step 执行产生唯一 opaque attemptId；retry 必须创建新的 Attempt；
- V1 通过 Attempt ID 和事件顺序隔离历史结果；fencing token 及其迟到写入保护列为 V2；
- V1 不把 Task/Shared Task List 作为独立公共执行对象；后续如引入 Task，必须定义稳定的 `task_id ↔ step_id` 关系；
- Step 转派、权限和状态迁移由运行时代码控制，不由模型自律保证。

### 规范事件

```text
Event
├── event_id
├── stream_id（run_id）
├── sequence（服务端分配；V1 仅内部使用，公共 cursor 列为 V2）
├── resource（type + id：run/plan_revision/step/attempt/member/approval/artifact/connector）
├── type（run.started / plan.created / step.ready / step.started /
│         step.completed / step.failed / attempt.started / attempt.completed /
│         attempt.failed / approval.required / artifact.created /
│         run.completed / run.failed / run.cancelled / plan.revised ...）
├── timestamp
└── data
```

要求：Event Log 是 allo 引擎内部的事实来源；Run/Step/Attempt 的持久化当前状态是 V1 的公共读取模型。事件持久化用于审计与诊断；公共 cursor 分页、断线追平和基于事件的投影重建列为 V2。不暴露 allo 内部事件形态与 provider 私有事件。V1 不要求服务端在进程重启后续跑原 Attempt。

每个状态迁移必须由合法 Command 产生并写入事件。事件序号由服务端分配；终态不能被迟到写入覆盖。

## 9. PluginSnapshot

```text
PluginSnapshot
├── snapshot_id / version
├── source_kind     # codebuddy-plugin | workbuddy-skill-market | workbuddy-connector-market | native
├── source_uri / source_path（相对定位，不写死绝对路径）
├── plugin_id / declared_version / resolved_revision
├── content_digest
├── materialized_root / data_root
├── components      # AgentDefinition[] / AgentTeamDefinition[] / SkillDefinition[] /
│                   # ConnectorDefinition[] / CommandDefinition[] / HookDefinition[] /
│                   # LspDefinition[] / CredentialSchema[] / PluginDependency[]
├── path_variables  # ${PLUGIN_ROOT} 等映射到 Agent Store 运行变量
├── imported_at
└── compatibility_report
```

规则：

- 快照生成后不可变；运行时只读快照，不读来源目录；
- 来源路径仅用于追溯，不进入对外 API 与公共定义。

## 10. 对象生命周期与版本快照

### 10.1 定义到运行的快照链

```text
PluginSnapshot（不可变来源快照）
        ↓
AgentDefinition / AgentTeamDefinition / SkillDefinition / ConnectorDefinition
        ↓ 解析为 allo Preset / ResolvedPresetSnapshot
ExecutionParticipant.preset_snapshot（运行前冻结）
        ↓
Run / TeamRun
        ↓
PlanRevision / Step / Attempt / Event / Artifact
```

规则：

- `PluginSnapshot` 生成后不可变；来源内容变化必须生成新 snapshot；
- Definition 更新生成新版本，不原地改变历史版本；
- Run 创建时冻结目标 Definition、Skill、Connector Policy 和成员配置；
- 正在运行的 Run 不因 Catalog 新版本发布而改变；
- 历史 Run 必须能够追溯到具体 snapshot、definition version 和 content digest；
- `AgentExecutionTemplate` 是 V1 TeamRun 的执行级固定成员快照，不是 TeamDefinition 的替代品；TeamRun 的规划上下文是本次运行的派生快照，不替代成员 Prompt Snapshot。

### 10.2 生命周期状态

```text
Definition: draft → imported → validated → enabled → disabled → superseded
Run: queued → starting → running → paused → completed | failed | cancelled | recovery_required
Step: pending → ready → in_progress → completed | failed | cancelled
Attempt: created → running → completed | failed | cancelled | stale
```

状态迁移由运行时代码控制；模型输出不能直接越权改变权限、任务归属或终态。

### 10.3 Team V1 约束

V1 TeamRun 必须支持：

- 固定成员 Participant 池；
- 由 Leader 规划指令、成员能力摘要和 Team 策略组成 Planning Context；
- Planner 根据 Planning Context 生成结构化 planned DAG；
- 依赖满足后的 ready Step 调度；
- 独立 ready Step 的局部并行；
- Step/Attempt 失败后的 retry 或 replan；
- Team、Step、Attempt、Event、Artifact 的持久化。

Leader 在此处是规划角色，不等同于必须持久化的独立 Conversation。成员的完整 Prompt 只绑定到各自 Participant/Attempt；Planning Context 只允许包含满足规划所需的脱敏能力摘要。
V1 不要求 Mailbox、成员自主认领、成员直连消息、长期成员会话和嵌套 Team。

## 11. 兼容性状态

| 状态 | 含义 |
|---|---|
| `compatible` | 可直接映射并按当前能力运行 |
| `compatible-with-adapter` | 需适配层/转换后运行 |
| `manual-review` | 需人工审查（安全/生命周期/SOP 未落地） |
| `unsupported` | 当前明确不支持，保留记录 |
| `pending-legal-review` | 版权/分发授权未确认，禁止公开分发 |

## 12. 一致性规则（DoD）

1. 任何新对象必须先在本模型登记，再进入 App Server/SDK/Web；
2. 公共 ID 一律 opaque，内部 ID 不出协议；
3. 凭据不出现于任何对外出口；
4. 不支持的来源组件必须可见标记，禁止静默丢弃；
5. 领域模型与 allo 内部模型之间只通过 Runtime Adapter 映射。