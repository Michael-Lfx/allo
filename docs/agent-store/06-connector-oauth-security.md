# Connector、OAuth 与安全模型

> 状态：架构冻结（Phase 0）；OAuth/安全链路待实现验证；发布阻断
> 日期：2026-08-26
> 前置：`01-domain-model.md`、`02-codebuddy-workbuddy-import-spec.md`、`04-allo-runtime-adapter.md`、`05-allo-app-server-protocol.md`
> 目标：定义 Connector 运行边界、凭据隔离、OAuth 全链路和高风险操作控制

## 1. 安全原则

### 1.0 入站调用方身份

V1 默认只支持本地可信进程：stdio 或 localhost WebSocket 建立 `LocalPrincipal`，主进程负责创建和绑定 `AuthContext`。Renderer 不直接持有调用方凭据，也不能自行声明 `principal_id`、issuer、audience 或 scopes。

`CallerIdentity`、`AuthContext` 与 `CredentialBinding` 是三个不同对象：

```text
CallerIdentity   = 谁在调用 Agent Store
AuthContext      = 本次协议连接被授予的权限
CredentialBinding = 运行时访问某个 Connector 的凭据引用
```

入站认证失败必须返回 `unauthenticated`、`invalid_issuer`、`invalid_audience` 或 `insufficient_scope`。本地 session reference 不得作为远程调用凭据复用。

### 1.1 两个 OAuth 方向必须隔离

```text
外部 Agent / MCP Client → Agent Store
    = 入站认证（caller identity）

Agent Store → 上游 MCP / API / CLI Connector
    = 出站认证（connector credential）
```

入站 Token 绝不转发为上游 Connector Token。两者分别绑定不同的 Principal、Issuer、Resource 和 Scope。

### 1.2 有效权限取交集

```text
caller
∩ agent
∩ team
∩ skill
∩ connector
∩ credential scopes
```

任何一个层级拒绝，最终操作都必须拒绝。模型 Prompt 不是权限边界。

Team 的 Planning Context 只能向 Planner 提供脱敏的成员能力摘要；成员完整 Prompt、凭据和未授权工具细节不得在 Leader 或成员之间共享。每个 Step 的实际权限仍由其 Participant 的快照和运行时策略交集决定。

### 1.3 默认拒绝高风险副作用

以下操作默认需要显式 Tool Policy，通常还需要 Approval：

```text
发送消息
发布内容
删除数据
修改外部记录
部署
支付
执行任意命令
写入工作区之外的文件
```

## 2. Connector 统一模型

### 2.1 Connector 类型

```text
remote-mcp    远程 Streamable HTTP/SSE MCP
stdio-mcp     本地 STDIO MCP 子进程
cli           受控 CLI 命令适配器
http-api      受控 HTTP API 适配器
composite     多步骤/多个底层能力组合
```

Connector 负责：

```text
安装状态
配置状态
授权状态
连接状态
健康检查
工具发现
工具过滤
凭据绑定
生命周期
审计
```

Skill 负责 Agent 使用能力的说明和流程；Connector 负责真实外部能力。不能因为一个连接器附带 `SKILL.md` 就把 Connector 和 Skill 合并。

### 2.2 Connector Runtime 接口

```ts
interface ConnectorRuntime {
  connect(ctx: ConnectorContext): Promise<void>;
  status(): Promise<ConnectorStatus>;
  listTools(): Promise<ConnectorTool[]>;
  callTool(
    name: string,
    args: unknown,
    ctx: ConnectorCallContext,
  ): Promise<ConnectorToolResult>;
  disconnect(): Promise<void>;
}
```

运行时必须：

- 只接收已校验的 ConnectorDefinition；
- 只暴露 allowlist 中的工具；
- 对工具名做 Connector 命名空间隔离；
- 在调用前重新计算有效策略；
- 记录脱敏审计事件；
- 不向模型或客户端返回凭据。

### 2.3 MCP 工具命名

公开工具名采用：

```text
connector__<connector_slug>__<tool_name>
```

例如：

```text
connector__github__search_issues
connector__mail__send_message
```

原始上游工具名只保留在内部映射中。工具过滤必须同时支持：

```text
connector allowlist
agent allowlist
team allowlist
caller exposure allowlist
```

## 3. 凭据模型

### 3.1 CredentialSchema

只声明需要什么，不保存真实值：

```text
CredentialSchema
├── id
├── connector_id
├── fields[]
│   ├── name
│   ├── kind          # apikey / oauth / token / env / secret
│   ├── required
│   ├── sensitive
│   └── validation
├── oauth_metadata
└── scopes
```

### 3.2 CredentialBinding

```text
CredentialBinding
├── id
├── principal_id
├── connector_id
├── issuer
├── resource_server
├── scopes[]
├── credential_ref       # 安全存储引用，不是明文
├── status
└── expires_at
```

禁止只用 `server_url` 作为全局凭据键。相同 URL 可能对应不同用户、Issuer、Resource 或 Scope。

### 3.3 凭据禁止出现的位置

真实 API Key、Access Token、Refresh Token、Client Secret 和 CLI 登录态不得进入：

```text
PluginSnapshot
AgentDefinition
SkillDefinition
Prompt
Tool Result
普通配置文件
日志
事件 data
Renderer 状态
SDK 公共响应
MCP Resource
```

本文档、测试数据和示例中出现凭据时统一写作：

```text
[REDACTED]
```

## 4. OAuth V1 范围

### 4.1 支持的标准子集

V1 支持：

```text
HTTPS
Streamable HTTP MCP
Authorization Code
PKCE S256
标准 Metadata Discovery
127.0.0.1 / localhost Loopback Callback
Token Refresh
授权后实际连接 Probe
```

复杂流程延期：

```text
自定义 URI Scheme
公网 Relay
无法登记 Loopback 的固定厂商 Callback
非标准 Token Endpoint
未经确认的动态注册
OAuth over STDIO
```

这些流程标记为 `unsupported-auth` 或 `manual-review`，不能通过普通 OAuth 状态显示为已连接。

### 4.2 OAuth 流程

```text
ConnectorDefinition 声明 OAuth/发现入口
    ↓
App Server 创建一次性 OAuth Login Session
    ↓
发现 Authorization Server Metadata
    ↓
生成 state + PKCE S256
    ↓
系统浏览器授权
    ↓
Loopback Callback
    ↓
校验 state、issuer、resource、redirect_uri
    ↓
交换 code
    ↓
凭据写入安全存储
    ↓
创建 CredentialBinding
    ↓
Connector Runtime 连接并 Probe
    ↓
返回 authenticated / connected / error 状态
```

浏览器回调由 Electron Main Process 或受信服务端处理，Renderer 不直接接收 Token。

### 4.3 Token 注入与刷新

完整运行链路必须是：

```text
login
  → token repository
  → Credential Provider
  → MCP transport
  → request Authorization header
  → 401/403 handling
  → refresh mutex
  → 一次重试
```

要求：

- 每个 Connector/Principal/Issuer 维度使用刷新锁，避免并发刷新产生 token 竞争；
- 401 只允许刷新并重试一次；
- 刷新失败将状态置为 `reauthorization_required`；
- 403 不应盲目刷新，应报告 Scope/权限不足；
- resource/issuer 不匹配时拒绝发送请求；
- Authorization header 不写日志、不进入事件 data。

“OAuth 登录成功”不等于“Connector 运行成功”。只有请求时注入、刷新和实际 Probe 全部通过，才能标记连接可用。

## 5. Connector 状态机

安装、配置、授权和连接必须分开显示：

```text
not_installed
    → installed
    → configured
    → authorization_required
    → authorizing
    → authenticated
    → connecting
    → connected
    → degraded | error | reauthorization_required
```

卸载或用户登出：

```text
connected/authenticated → logged_out → configured/installed
```

`authenticated` 只表示凭据已被安全存储并通过基本校验；只有真实 transport 注入和 Probe 成功后才能进入 `connected`。刷新失败必须进入 `reauthorization_required`，不得继续使用未知有效性的旧 Token。

状态迁移由 Connector Manager 控制；连接配置存在不代表工具 ready，必须执行实际健康检查。

## 6. CLI Connector 安全边界

CLI Connector 不是任意 Shell 代理。每个 CLI Connector 必须声明：

```text
allowed_executable
allowed_subcommands
argument_schema
working_directory_policy
environment_allowlist
network_policy
timeout
max_output_size
side_effect_level
approval_policy
```

运行规则：

- 禁止由模型拼接任意命令字符串；
- 参数必须通过 schema 校验并按 argv 数组传递；
- 工作目录必须位于受控 workspace；
- 环境变量使用 allowlist，敏感值从 Credential Provider 注入；
- 超时必须终止/回收子进程并记录结果；
- 输出截断，禁止把环境变量和凭据写入日志；
- 高风险子命令要求 Approval；
- 无受控适配器时，市场 CLI 只能导入为 `manual-review`。

## 7. STDIO MCP 安全边界

STDIO Connector 通过受控子进程运行：

- 命令和参数来自已校验 descriptor；
- 禁止工具调用覆盖 executable、cwd 和基础环境；
- 子进程使用独立 stdin/stdout 管道；
- stderr 脱敏后写审计；
- 子进程崩溃、超时和协议错误进入 Connector 状态；
- 进程退出后不得继续接受工具调用；
- 工作区、网络和文件权限按 Connector Policy 限制。

## 8. 安全审计事件

至少记录：

```text
connector.installed
connector.configured
connector.auth.started
connector.auth.completed
connector.auth.failed
connector.connected
connector.disconnected
connector.tool.listed
connector.tool.call.started
connector.tool.call.completed
connector.tool.call.denied
connector.token.refresh.failed
connector.process.timeout
approval.requested
approval.resolved
```

审计记录只保存：

```text
audit_id
principal_id
connector_id
tool_name
policy_decision
argument_summary（脱敏）
result_summary（脱敏）
run_id
created_at
```

不得保存完整 Token、完整 Authorization header、Client Secret 或未经脱敏的工具参数。

## 9. V1 验收用例

- `TC-SEC-001`：入站 Token 不会被作为上游 Connector Token 使用；
- `TC-SEC-002`：有效权限按 caller/agent/team/skill/connector/credential scopes 取交集；
- `TC-SEC-003`：MCP 工具只暴露命名空间 allowlist；
- `TC-OAUTH-001`：标准 PKCE Loopback OAuth 登录成功；
- `TC-OAUTH-002`：Token 不进入 Renderer、日志、Prompt、事件和公共响应；
- `TC-OAUTH-003`：请求时注入 Authorization，401 刷新后只重试一次；
- `TC-OAUTH-004`：Issuer/Resource 不匹配时请求被拒绝；
- `TC-OAUTH-005`：刷新失败进入 reauthorization_required；
- `TC-CONN-001`：Connector 配置存在但 Probe 失败时不能显示 connected；
- `TC-CLI-001`：任意命令、参数和工作目录覆盖均被拒绝；
- `TC-STDIO-001`：STDIO 进程崩溃/超时后工具调用停止且状态可观测；
- `TC-AUDIT-001`：高风险调用有脱敏审计记录和必要 Approval。

## 10. 发布准入

Connector/OAuth 能力只有在以下条件全部满足后进入 V1 发布：

1. 至少一个标准远程 MCP Connector 通过连接、工具发现、调用和断线测试；
2. 至少一个需要 OAuth 的 Connector 通过登录、存储、注入、刷新和 Probe；
3. 凭据泄露扫描通过；
4. Tool Policy 和命名空间过滤通过；
5. CLI/STDIO 未绕过进程、目录、环境和网络边界；
6. 审计记录可查询且已脱敏；
7. 复杂 OAuth 被明确标记为 unsupported-auth，不进入默认安装流程。
