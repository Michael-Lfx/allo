# Flowy / Agent Store Web 集成规格

> 状态：架构冻结（Phase 0）；Web/Flowy 待实现验证；发布阻断
> 日期：2026-08-26
> 前置：`05-allo-app-server-protocol.md`、`06-connector-oauth-security.md`、`07-typescript-sdk.md`、`10-public-contracts.md`
> 目标：定义 Web/Flowy 如何通过 SDK 使用 Agent Store，不直接依赖 allo 内部实现

## 1. 集成边界

```text
Flowy/Web UI
    ↓ @flowy-agent-store/client / @flowy-agent-store/react
TypeScript SDK
    ↓ App Server Protocol
Agent Store App Server
    ↓ Runtime Adapter
allo Runtime
```

Web/Flowy 不得直接访问：

```text
allo 内部 REST
allo UI WebSocket
allo 数据库
Execution/Participant 内部对象
MCP 上游服务
安全凭据存储
```

UI 展示的是 Agent Store 公共模型：

```text
Agent
Team
Skill
Connector
Run
Plan
Step
Attempt
Event
Artifact
Approval
```

这里的 `Agent` 指 Agent Store AgentDefinition/allo Preset，不是 Claude Code、Codex 等 Runtime Agent 实例。Runtime Agent 由 App Server 根据已解析的 Preset 和运行策略选择，Web/Flowy 不直接管理其内部身份。

## 2. 页面信息架构

### 2.1 Catalog

```text
/agents
/agents/:id
/teams
/teams/:id
/skills
/connectors
/connectors/:id
```

Catalog 页面必须显示：

```text
名称
版本
来源
兼容性状态
启用/禁用状态
所需权限摘要
可运行能力
最后验证时间
```

不得在列表中显示真实凭据或完整隐藏 Prompt。

### 2.2 Agent Detail

Agent 详情包括：

```text
身份与描述
版本与来源
绑定 Skill
可用 Connector 工具摘要
模型摘要
权限与风险摘要
兼容性报告
运行入口
```

“运行 Agent”按钮只提交 App Server 请求，不在浏览器本地拼装 Agent Prompt。

### 2.3 Team Detail

Team 页面必须明确区分：

```text
Team Definition
V1 Runtime Capabilities
成员名册
Leader（规划角色）
成员能力摘要（脱敏）
规划策略
并发限制
工具与权限摘要
兼容性状态
```

V1 Team 能力展示：

```text
固定成员
Planning Context 驱动 planned DAG
局部并行
retry
replan
事件
Artifact
```

未实现能力不得显示为已启用：

```text
Mailbox
成员自主认领
成员直连消息
长期成员会话
嵌套 Team
```

### 2.4 Run Launch

单 Agent 和 Team 共用启动流程，但 Team 额外展示：

```text
Team 成员预览
Leader（规划角色）
Planning Context 摘要
planning policy
max parallel
workspace
Connector 权限摘要
```

提交前显示有效配置摘要：

```text
Preset/Definition version
Skill versions
Connector versions
workspace
approval policy
effective max parallel
```

此摘要是用户确认依据，不包含真实凭据。

### 2.5 Run Workspace

```text
/runs/:id
/runs/:id/plan
/runs/:id/timeline
/runs/:id/artifacts
/runs/:id/approvals
```

TeamRun 详情页应展示本次运行的 `planning_context_digest`，但不展示 Planning Context 正文或成员完整 Prompt。

Run 页面布局：

```text
┌───────────────────────────────────────────┐
│ Run Header：状态 / Team / 取消 / 暂停      │
├───────────────┬───────────────────────────┤
│ Plan / DAG     │ 当前 Step / Attempt        │
│               │ Agent 输出 / 工具摘要       │
├───────────────┴───────────────────────────┤
│ Timeline / Events / Approvals / Artifacts  │
└───────────────────────────────────────────┘
```

## 3. Plan/DAG 展示

DAG 节点至少显示：

```text
step_id
标题/目标
绑定 Agent/Participant
状态
依赖
当前 Attempt
开始/结束时间
失败原因摘要
输出 Artifact
```

状态使用公共模型：

```text
pending
ready
in_progress
completed
failed
cancelled
```

禁止 UI 根据节点位置猜测依赖；依赖必须来自 App Server 的结构化字段。

Plan Revision 必须保留历史：

```text
Plan v1
Plan v2（replan）
Plan v3（replan）
```

用户可以查看差异，但不能从 UI 直接修改历史 Plan。

## 4. Timeline 与事件消费

UI 通过 SDK 的 `run.follow` 订阅事件：

```text
进入 Run 详情页
    ↓
run/get 拉取持久化状态
    ↓
订阅实时通知（尽力而为）
    ↓
收到通知 → 重新 run/get 合并
    ↓
按 event_id 去重展示
    ↓
断线重连后重新 run/get 对齐
```

UI Store 不应只保存当前屏幕可见事件。至少保留：

```text
run status
plan revisions
step status
attempt status
approval status
artifact refs
last error
```

高频 Token 流只作为可选展示数据，不作为公共状态持久化的唯一依据。

## 5. Connector 页面与 OAuth

### 5.1 Connector Detail

显示：

```text
Connector 名称和版本
类型
来源
安装状态
配置状态
授权状态
连接状态
工具 allowlist
风险等级
最后 Probe 时间
兼容性状态
```

状态分开显示：

```text
installed
configured
authorization_required
authenticated
connected
degraded
error
reauthorization_required
```

### 5.2 OAuth 交互

```text
点击连接
    ↓
SDK connector.auth.start
    ↓
打开 authorization_url
    ↓
主进程/服务端处理 Callback
    ↓
UI 轮询或订阅 auth status
    ↓
显示账号摘要与 connected 状态
    ↓
connector.test / Probe
```

UI 不接收或保存：

```text
Authorization Code
Access Token
Refresh Token
Client Secret
完整 Authorization Header
```

OAuth 失败应显示可操作原因：

```text
authorization_required
scope_insufficient
issuer_mismatch
resource_mismatch
callback_timeout
token_refresh_failed
connector_probe_failed
```

## 6. Approval UI

Approval 卡片必须显示：

```text
请求来源 Run/Step/Agent
Connector 和 Tool
脱敏参数摘要
副作用等级
目标资源摘要
过期时间
批准/拒绝按钮
```

不显示：

```text
Token
完整敏感参数
不必要的系统 Prompt
内部数据库 ID
```

批准只是一次具体操作的授权。服务端仍需重新计算有效策略和参数绑定。

## 7. Artifact UI

Artifact 列表：

```text
名称
类型
大小
sha256
来源 Run/Step
创建时间
读取/下载操作
```

浏览器只通过 App Server 的 Artifact API 获取内容，不能把任意本地路径传给服务端。

下载前必须校验：

```text
artifact_id
run_id
workspace_id
权限
文件 digest
```

## 8. Flowy 集成方式

### 8.1 主进程职责

Flowy/Electron 主进程负责：

- 启动或连接本地 App Server；
- 配置 WebSocket/stdio Transport；
- 处理 OAuth 打开浏览器与回调；
- 访问操作系统安全存储；
- 向 Renderer 暴露最小受控 API；
- 处理窗口、通知和系统级生命周期。

### 8.2 Renderer 职责

Renderer 负责：

- 使用 SDK Catalog；
- 启动和观测 Run；
- 呈现 Plan/Timeline/Artifact/Approval；
- 展示 Connector/OAuth 状态；
- 不保存凭据，不直接调用外部 Connector。

### 8.3 页面与 SDK 映射

| 页面能力 | SDK 方法 |
|---|---|
| Agent Catalog | `client.agents.list/get` |
| Team Catalog | `client.teams.list/get` |
| Agent 运行 | `client.runs.agent` |
| Team 运行 | `client.runs.team` |
| Run 状态 | `client.runs.get` |
| Timeline | `client.runs.follow/events` |
| 暂停/恢复/取消 | `client.runs.pause/resume/cancel` |
| Retry/Replan | `client.runs.retry/replan` |
| Artifact | `client.artifacts.list/get` |
| Approval | `client.approvals.respond` |
| OAuth | `client.connectors.authStart/authStatus/logout` |

## 9. 前端状态模型

建议按资源拆分 Store：

```text
catalogStore
runStore
planStore
eventStore
artifactStore
approvalStore
connectorStore
sessionStore
```

Run Store 的最小结构：

```ts
interface RunViewState {
  run: RunDetail | null;
  status: "loading" | "ready" | "stale" | "reconnecting" | "error";
  lastEventId?: string; // 通知去重辅助，可选
  planRevisions: PlanRevision[];
  stepsById: Record<string, StepView>;
  attemptsById: Record<string, AttemptView>;
  pendingApprovals: Approval[];
  artifacts: ArtifactSummary[];
  lastError?: PublicError;
}
```

Store 以服务端持久化状态为准，事件通知只触发刷新。呈现 replan 相关变化时，不能只覆盖当前 DAG，必须保存历史 Plan Revision。

## 10. 权限与路由守卫

前端按钮可根据 capability 隐藏或禁用，但安全控制必须在 App Server/Runtime 重做。

路由守卫至少覆盖：

```text
未初始化
协议版本不兼容
资源不存在
资源 disabled
兼容性 blocked
无运行权限
Connector 未授权
Workspace 不可用
```

“按钮不可见”不等于“操作安全”。

## 11. 验收与实现顺序

Web/Flowy 测试用例的唯一正文位于 `agent-store-v1-test-cases.md`，本文件只定义页面与主进程边界。实现顺序由路线图统一管理：

```text
SDK Provider → Catalog → Run Workspace → Plan/Timeline → Approval/Artifact → Connector 状态
```

必须通过 `TC-WEB-001` 至 `TC-WEB-010`；页面不得自行推断状态或绕过 App Server 安全校验。