# Agent Store TypeScript SDK 规格

> 状态：架构冻结（Phase 0）；SDK 待实现验证；发布阻断
> 日期：2026-08-26
> 前置：`01-domain-model.md`、`05-allo-app-server-protocol.md`、`06-connector-oauth-security.md`、`10-public-contracts.md`
> 目标：提供 App Server Protocol 的 typed client；SDK 不直接依赖 allo 内部实现

## 1. 定位与边界

```text
TypeScript SDK
    ↓ Versioned App Server Protocol
Agent Store App Server
    ↓ Runtime Adapter
allo Runtime
```

SDK 负责：

- 连接和初始化 App Server；
- 暴露协议定义对应的 TypeScript 类型；
- 调用 Catalog、Thread、Run、Event、Artifact、Approval、Connector API；
- 处理请求 ID、幂等键、重连和结构化错误；
- 为 Node.js、Electron 主进程和浏览器 Web 提供合适的 Transport。

SDK 不负责：

- 定义 Agent/Team/Run 的业务语义；
- 直接访问 allo REST、WebSocket 或数据库；
- 保存真实 OAuth Token；
- 执行本地任意命令；
- 绕过 App Server 的 Tool Policy、Approval 和权限校验。

## 2. 包结构

推荐拆分：

```text
@agent-store/protocol
@agent-store/client
@agent-store/node
@agent-store/browser
@agent-store/react
```

### 2.1 `@agent-store/protocol`

只包含：

```text
Request/Response/Notification 类型
Initialize 类型
Catalog 类型
Thread/Turn/Run 类型
Plan/Step/Attempt/Event 类型
Approval/Artifact 类型
Connector/OAuth 类型
错误码和错误结构
```

该包不得依赖 Node.js、Electron、浏览器 DOM 或 allo Rust 类型。

### 2.2 `@agent-store/client`

包含：

```text
AppServerClient
AgentClient
TeamClient
SkillClient
ConnectorClient
ThreadClient
RunClient
EventStreamClient
ArtifactClient
ApprovalClient
```

### 2.3 Transport

```ts
export interface Transport {
  connect(): Promise<void>;
  request<T>(method: string, params: unknown): Promise<T>;
  notify(method: string, params: unknown): Promise<void>;
  onNotification(listener: (message: unknown) => void): () => void;
  close(): Promise<void>;
}
```

V1 实现：

```text
StdioTransport：Node.js/CLI
WebSocketTransport：Electron/Web/Flowy
```

浏览器 SDK 不直接实现 stdio，也不持有客户端 Secret。

## 3. AppServerClient

```ts
export interface AppServerClientOptions {
  transport: Transport;
  client: ClientInfo;
  capabilities?: ClientCapabilities;
  protocolVersion?: string;
  requestTimeoutMs?: number;
  reconnect?: ReconnectOptions;
}

export class AppServerClient {
  initialize(): Promise<InitializeResult>;
  close(): Promise<void>;
  isReady(): boolean;
  readonly agents: AgentClient;
  readonly teams: TeamClient;
  readonly skills: SkillClient;
  readonly connectors: ConnectorClient;
  readonly threads: ThreadClient;
  readonly runs: RunClient;
  readonly artifacts: ArtifactClient;
  readonly approvals: ApprovalClient;
}
```

初始化顺序：

```text
connect transport
    → initialize
    → 校验协议版本与 capabilities
    → initialized notification
    → ready
```

初始化失败不得调用业务方法。协议版本不兼容时返回明确错误，不静默忽略字段。

## 4. Catalog Client

### 4.1 AgentDefinition 与 Team

```ts
// AgentId identifies an Agent Store AgentDefinition, not a Runtime Agent.
interface AgentClient {
  list(input?: ListInput): Promise<Page<AgentSummary>>;
  get(id: AgentId, version?: string): Promise<AgentDetail>;
}

interface TeamClient {
  list(input?: ListInput): Promise<Page<TeamSummary>>;
  get(id: TeamId, version?: string): Promise<TeamDetail>;
  run(input: TeamRunInput): Promise<RunReceipt>;
}
```

`TeamDetail.teamRuntimeCapabilities` 必须由服务端返回，SDK 不自行推断完整 Team 能力。Team 详情中的 Leader 是规划角色；单次 TeamRun 的 `planningContextDigest` 只从 Run 查询结果读取，不混入静态 Team 定义。

### 4.2 Skill 与 Connector

```ts
interface SkillClient {
  list(input?: ListInput): Promise<Page<SkillSummary>>;
  get(id: SkillId, version?: string): Promise<SkillDetail>;
}

interface ConnectorClient {
  list(input?: ListInput): Promise<Page<ConnectorSummary>>;
  get(id: ConnectorId, version?: string): Promise<ConnectorDetail>;
  status(id: ConnectorId): Promise<ConnectorStatus>;
  test(id: ConnectorId): Promise<ConnectorProbeResult>;
  authStatus(id: ConnectorId): Promise<OAuthStatus>;
  authStart(id: ConnectorId): Promise<OAuthStartResult>;
  logout(id: ConnectorId): Promise<void>;
}

interface ArtifactClient {
  list(runId: RunId, input?: ListInput): Promise<Page<ArtifactSummary>>;
  get(runId: RunId, artifactId: ArtifactId): Promise<ArtifactDetail>;
  download(runId: RunId, artifactId: ArtifactId): Promise<ReadableStream<Uint8Array>>;
}

interface ApprovalClient {
  list(runId: RunId, input?: ListInput): Promise<Page<ApprovalSummary>>;
  get(runId: RunId, approvalId: ApprovalId): Promise<ApprovalDetail>;
  respond(input: ApprovalResponseInput): Promise<ApprovalReceipt>;
}
```

OAuth 客户端只获得：

```text
auth session id
authorization URL
状态
错误
账号摘要
```

不得获得真实 Token。

## 5. Run Client

```ts
interface RunClient {
  agent(input: AgentRunInput): Promise<RunReceipt>;
  team(input: TeamRunInput): Promise<RunReceipt>;
  get(id: RunId): Promise<RunDetail>;
  result(id: RunId): Promise<RunResult>;
  cancel(id: RunId): Promise<CancelReceipt>;
  pause(id: RunId): Promise<RunControlReceipt>;
  resume(id: RunId): Promise<RunControlReceipt>;
  retry(id: RunId, input?: RetryInput): Promise<RunReceipt>;
  replan(id: RunId, input?: ReplanInput): Promise<PlanRevisionReceipt>;
  events(id: RunId, input?: EventQuery): Promise<EventPage>;
  follow(id: RunId, input?: FollowOptions): EventSubscription;
}
```

### 5.1 Run 输入

Agent Store `agentId` 标识 AgentDefinition/Preset，不标识 Claude Code、Codex 等 Runtime Agent；Runtime Agent 由服务端根据 Preset 快照和运行策略选择。

```ts
interface AgentRunInput {
  agentId: AgentId;
  agentVersion?: string;
  input: UserInput;
  workspaceId?: WorkspaceId;
  idempotencyKey: string;
}

interface TeamRunInput {
  teamId: TeamId;
  teamVersion?: string;
  goal: string;
  workspaceId?: WorkspaceId;
  planning?: {
    mode?: "planned";
    adaptationPolicy?: "fixed" | "adaptive";
    planGate?: "automatic" | "approval";
    maxParallel?: number;
  };
  idempotencyKey: string;
}

interface ApprovalResponseInput {
  requestId: string;
  approvalId: ApprovalId;
  decision: "approve" | "reject";
  idempotencyKey: string;
}
```

服务端仍然对所有参数进行安全策略裁剪，客户端参数不是最终授权。

## 6. EventStreamClient

```ts
interface EventSubscription {
  onEvent(listener: (event: CanonicalEvent) => void): () => void;
  onError(listener: (error: SdkError) => void): () => void;
  onEnd(listener: (result: StreamEnd) => void): () => void;
  pause(): void;
  resume(): void;
  close(): Promise<void>;
  readonly runId: string;
}
```

事件处理规则：

- 通知只是尽力而为的刷新信号：允许丢失、重复与乱序；
- 收到通知后以 runs.get() 拉取权威状态并合并；
- 按 `event_id` 去重展示；
- 断线重连后不做历史事件补发，直接重新拉取状态；
- 不能把本地 UI 状态当作事件事实来源；
- 终态以 run.result 为准，通知仅为提前刷新。

推荐消费模式：

```ts
const stream = client.runs.follow(runId);
stream.onEvent(event => dedupeByEventId(event, store)); // 通知触发刷新
stream.onError(() => scheduleReconnect());             // 重连后重新拉取状态
```

## 7. 错误模型

```ts
class AppServerError extends Error {
  code: ErrorCode;
  requestId?: string;
  retryable: boolean;
  details?: unknown;
}

class TransportError extends Error {
  phase: "connect" | "send" | "receive" | "close";
  retryable: boolean;
}

class ProtocolError extends Error {
  kind: "invalid_message" | "version_mismatch" | "unexpected_response";
}
```

SDK 不应把错误 message 当作业务判断依据；应使用稳定的 `code`。

重试建议：

```text
连接失败             → 有界指数退避
429/暂时不可用        → 依据 retryable 和服务端建议
idempotency_conflict  → 不自动重试
policy_denied         → 不重试
approval_required     → 转 UI/调用方处理
attempt_stale         → 重新获取 Run 状态
```

不得自动重试有副作用的工具调用，除非 App Server 明确返回可安全重试的幂等语义。

## 8. React 集成

`@agent-store/react` 只封装服务器状态，不隐藏协议语义：

```ts
useAgents(query)
useTeams(query)
useRun(runId)
useRunNotifications(runId)
useConnectorStatus(connectorId)
useArtifact(artifactId)
```

Hook 必须支持：

```text
loading
error
stale
reconnecting
terminal
```

通知订阅建议使用外部 store，避免组件卸载导致重复建连。缓存键必须包含：

```text
protocol_version
resource_type
resource_id
resource_version
```

## 9. Node/Electron 与 Browser 边界

### Node/Electron Main

可负责：

- 启动/连接 stdio App Server；
- 处理 OAuth 浏览器流程；
- 访问安全凭据存储；
- 向 Renderer 提供受控 RPC。

### Browser/Renderer

只负责：

- Catalog 展示；
- Run 启动和观测；
- Approval UI；
- OAuth URL 打开和状态展示；
- Artifact 元数据和受控读取。

禁止：

- 读取安全存储；
- 拼接 Authorization header；
- 直接启动 stdio/CLI；
- 直接调用上游 MCP；
- 将 Token 写入 localStorage/sessionStorage。

## 10. 版本与兼容策略

- Protocol version 是服务端和客户端协商的公共版本；
- SDK minor 版本可以增加可选字段；
- 新增事件类型必须允许旧客户端安全忽略；
- 删除字段、改变状态语义和修改错误码含义需要 major 版本；
- `@agent-store/protocol` 与 `@agent-store/client` 的版本必须记录生成来源；
- SDK 不将 allo crate 版本当作公共协议版本。

## 11. 验收与实现顺序

SDK 测试用例的唯一正文位于 `agent-store-v1-test-cases.md`，本文件只声明 SDK 范围。实现顺序由路线图统一管理：

```text
Protocol Schema → generated types → Transport → Clients → reconnect → React wrapper
```

SDK 必须通过 `TC-SDK-001` 至 `TC-SDK-010`；不要在此处复制测试步骤。