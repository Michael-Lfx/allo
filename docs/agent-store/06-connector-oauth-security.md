# Connector、OAuth 与安全模型

> 状态：架构基线（Phase 0；发版前可改，非冻结，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 4）；OAuth 链路已实证（本文件 §12）；发布阻断
> 日期：2026-08-26
> 前置：`01-domain-model.md`、`02-codebuddy-workbuddy-import-spec.md`、`04-allo-runtime-adapter.md`、`05-allo-app-server-protocol.md`
> 目标：定义 Connector 运行边界、凭据隔离、OAuth 全链路和高风险操作控制

## 1. 安全原则

### 1.0 入站调用方身份

V1 默认只支持本地可信进程：由 **localhost WebSocket** 建立 `LocalPrincipal`（V1 不含 stdio，见 `16-sdk-webui-site-priority-plan.zh.md` §7 决策 2），主进程负责创建和绑定 `AuthContext`。Renderer 不直接持有调用方凭据，也不能自行声明 `principal_id`、issuer、audience 或 scopes。

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

---

## 11. 验收用例正文（TC-OAUTH / TC-CONN / TC-CLI / TC-STDIO）

> 本节由 `agent-store-v1-test-cases.md` 原 §7 并入（2026-09-11 文档合并）；TC 编号与用例正文保持不变，内部分节号沿用原文。§9 是摘要索引，本节是逐条正文。

### 7. Connector 与 OAuth

#### TC-CONN-001：工具命名空间

- 等级：P0
- 断言：上游工具只能以公开命名空间名称暴露；未在 allowlist 的工具不能调用

#### TC-OAUTH-001：标准 PKCE Loopback

- 等级：P0
- 断言：state、PKCE、issuer、resource、redirect_uri 校验通过；成功后得到 authenticated 状态

#### TC-OAUTH-002：凭据隔离

- 等级：P0
- 断言：Access/Refresh Token 不进入 Renderer、Prompt、Tool Result、Event、日志、SDK 公共响应

#### TC-OAUTH-003：请求时注入与刷新

- 等级：P0
- 断言：请求时由 Credential Provider 注入；401 触发一次刷新和一次重试；刷新失败进入 reauthorization_required

#### TC-OAUTH-004：错误边界

- 等级：P0
- 操作：Issuer/Resource 不匹配、403、Callback 超时
- 断言：分别返回明确错误；不盲目刷新或发送请求

#### TC-CONN-002：Connector Probe

- 等级：P1
- 断言：配置存在但 Probe 失败时状态不能显示 connected

#### TC-CLI-001：CLI 受控执行

- 等级：P0
- 断言：任意 executable、subcommand、argv、cwd、环境变量覆盖均被拒绝；仅允许 schema/allowlist 内输入

#### TC-STDIO-001：STDIO 生命周期

- 等级：P0
- 断言：子进程超时、崩溃或协议错误后被回收；工具调用停止；状态和审计可查询


---

## 12. 附录 A · OAuth 运行时证据（TC-OAUTH-001/002/004）

> 本节由 `oauth-runtime-evidence.zh.md` 整体并入（2026-09-11）。原文的时间戳与「历史实测快照，非契约」定性**保持不变**。


> 日期：2026-09-09
> 脚本：`web/scripts/sdk-live-oauth.ts`（可重复门禁脚本）
> 运行：`AGENT_STORE_BIN=target/debug/agent-store.exe bun scripts/sdk-live-oauth.ts`
> 结果：**26/26 PASS**
> 范围：WP-3 P0-C/D 的 OAuth 运行时部分。协议面全部经 SDK 公共面
> （`launchClient` + `client.connectors.*`）；宿主管理面（MCP enable）标注 `[host admin]`。

### 1. 环境与 mock 平台

本地 `Bun.serve`（127.0.0.1 回环）同时扮演 MCP 资源服务器与授权服务器，
按资源路径分四个场景；四个连接器经本地市场目录导入并安装：

| 场景 | 资源 URL | 行为 | 用途 |
|---|---|---|---|
| good | `/good/mcp` | RFC 9728 → RFC 8414 → RFC 7591 动态注册 → PKCE S256 → 302 回环回调 → token 交换（服务端校验 `code_verifier`）→ 受 Bearer 保护的 `echo` 工具 | TC-OAUTH-001/002 |
| forbidden | `/forbidden/mcp` | 授权成功，但资源恒 403 | TC-OAUTH-004（403 边界） |
| mismatch | `/mismatch/mcp` | 保护资源元数据指向不发布 RFC 8414 元数据的授权服务器 | TC-OAUTH-004（issuer/resource 不匹配） |
| timeout | `/timeout/mcp` | authorize 返回 200 且不重定向 | TC-OAUTH-004（回调超时） |

说明：宿主 `login` 的浏览器步由 `open::that` 触发——本机默认浏览器会短暂打开
（good/forbidden 自动 302 回环并显示成功页；timeout 停在 mock 提示页），属预期。

### 2. 用例证据

#### TC-OAUTH-001 标准 PKCE Loopback（8/8）

| 检查 | 结果 |
|---|---|
| `OA-001.pre.status` | `authorization_required`（未授权时的组合状态） |
| `OA-001.pre.unauthenticated` | `{"state":"not_authenticated"}` |
| `OA-001.auth-start-ack` | `{"state":"started"}`（宿主异步执行，客户端轮询） |
| `OA-001.authenticated` | `{"state":"authenticated"}` |
| `OA-001.rfc7591-registration` | 动态注册命中 1 次 |
| `OA-001.pkce-s256-challenge` | authorize 命中 1 次，`code_challenge_method=S256` 1 次 |
| `OA-001.token-exchange-authcode` | authorization_code 授权成功 1 次 |
| `OA-001.pkce-verifier-validated` | mock 端 `code_verifier` 校验失败 0 次 |
| `OA-001.probe-with-injected-token` | `{"success":true,"tools":["echo"]}`（**请求时 Bearer 注入生效**） |
| `OA-001.post.connected` | `connected` |

`state`/`redirect_uri` 校验由宿主回环回调强制（路径 + CSRF），`issuer`/`resource`
由 RFC 9728 → RFC 8414 发现链确定；PKCE 由 mock token 端点按 `S256(code_verifier)`
与 authorize 记录比对强制，任一环节不符即登录失败。

#### TC-OAUTH-002 凭据隔离（2/2）

| 检查 | 结果 |
|---|---|
| `OA-002.no-token-in-public-surfaces` | 5 个公共面（`auth/status`、`connector/get`、`connector/status`、`connector/list`、`connector/test`）扫描 `oa-access-good` / `oa-refresh-good` / 本次签发的全部 token → 命中 0 |
| `OA-002.issued-tokens-scanned` | 本次共签发 3 个凭据串，全部纳入扫描 |

#### TC-OAUTH-004 错误边界（10/10）

| 检查 | 结果 |
|---|---|
| `OA-004-mismatch.never-authenticated` | 发现阶段失败 → `not_authenticated`（永不误标已授权） |
| `OA-004-mismatch.no-token-request` | `{"authCode":0,"refresh":0}`（**未发起任何 token 请求**） |
| `OA-004-mismatch.no-authorize` | authorize 命中 0（发现失败即止，不打开浏览器） |
| `OA-004-mismatch.probe-fails` | 探针 `success:false` |
| `OA-004-forbidden.authenticated` | 授权本身成功 → `authenticated` |
| `OA-004-forbidden.403-clear-error` | `{"success":false,"error":"HTTP 403 Forbidden from server"}`（明确错误） |
| `OA-004-forbidden.no-blind-refresh` | refresh 命中 0（**403 不触发盲目刷新**） |
| `OA-004-forbidden.token-intact` | 探针失败后凭据仍为 `authenticated`（未被误清） |
| `OA-004-timeout.not-authenticated` | 120s 回调超时后仍 `not_authenticated` |
| `OA-004-timeout.no-token-request` | `{"authCode":0,"refresh":0}` |
| `OA-004-timeout.probe-fails` | 探针 `success:false`（未连接的连接器不报 connected） |

宿主侧对应日志（同一 data dir `logs/*.nomi.log`）：

```text
WARN "MCP OAuth login failed" url="…/mismatch/mcp" error="OAuth error: authorization server …/dead-oauth does not publish RFC 8414 metadata; dynamic client registration is not supported — a pre-registered client id (MCP_OAUTH_CLIENT_ID) is required"
WARN "MCP OAuth login failed" url="…/timeout/mcp" error="OAuth error: OAuth callback timed out — no redirect received within 120s"
INFO "MCP OAuth login completed" url="…/good/mcp"
INFO "MCP OAuth login completed" url="…/forbidden/mcp"
```

### 3. 本次 live 逼出的缺陷与修复

#### B7：并发 `login` 覆盖共享 pending 槽（已修）

首跑把 timeout 场景预启动、紧接着跑 good 场景，结果 good 登录失败：

```text
WARN "MCP OAuth login failed" url="…/good/mcp" error="OAuth error: CSRF state mismatch"
```

根因：`McpOAuthService.pending` 是单个 `Arc<Mutex<Option<PendingLogin>>>`
（设计注释即写明「同一时刻仅一个 login」），但 `connector/auth/start` 在后台
`tokio::spawn` 中执行 `login`，没有任何门闩——两个并发流程互相覆盖 pending，
先到的好回调只能拿到后完成流程的 CSRF 值 → state 校验失败。

修复：`login` 增加 `Arc<tokio::sync::Mutex<()>>` 串行门闩（同一 service 的所有
clone 共享），并补回归测试
`concurrent_logins_on_shared_service_both_succeed`（两个并发 login 均须成功）。
副作用：并发登录改为串行（后一个等待前一个完成，含最长 120s 回调窗口）；
客户端仍立即收到 `started` 应答，属可接受行为。

#### B8：动态注册身份未落盘（已修）

live 保留库中 token 行引用了 `registration_id`，但 `oauth_client_registrations`
表为空——注册只落在 InMemory 回退仓库：

```text
oauth_tokens: (…/good/mcp, oa-access-good, oa-refresh-good, registration_id=1, …)
oauth_client_registrations: <空>
```

根因：`crates/backend/nomifun-app/src/router/state.rs` 构建 MCP router 的
`McpOAuthService` 时未挂 `SqliteOAuthClientRegistrationRepository`
（`AppServices` 侧挂了），落到进程内回退仓库。后果：重启后
`refresh_token()` 查不到注册行 → `reauthorization_required`，且每次重启会
重新动态注册、旧 token 成为孤儿。

修复：`state.rs` 与 `AppServices` 使用同一 `SqliteOAuthClientRegistrationRepository`，
`connection_test` 与 `oauth_service` 共享该实例。修复后复跑，注册行落盘并与 token 正确关联：

```text
oauth_tokens:            (…/good/mcp, oa-access-good, registration_id=1)
                         (…/forbidden/mcp, oa-access-forbidden, registration_id=2)
oauth_client_registrations: (1, …/good/mcp, dyn-good, dynamic)
                            (2, …/forbidden/mcp, dyn-forbidden, dynamic)
                            (3, …/timeout/mcp, dyn-timeout, dynamic)
```

### 4. 观察与边界

- **宿主侧存储**：`oauth_tokens` 当前直存访问/刷新 token（`nomifun-mcp` 设计
  文档标注「应加密存储」）。属宿主存储层事项，不在 TC-OAUTH-002 的公共面范围内，
  建议另立任务跟踪。
- **回调拒收行为**：路径/state 不符时回环回调直接断连（不回 4xx），浏览器会看到
  连接错误页；安全上可接受（不向可疑请求回显信息），但 HTTP 语义不完整。
  两条既有集成测试（`callback_state_mismatch_is_rejected` /
  `callback_path_mismatch_is_rejected`）因 Windows 下 hyper 报
  `IncompleteMessage` 而失败，本次改为容忍传输层拒收（断言仍校验登录失败）。
- **未覆盖**：TC-OAUTH-003（401 → 刷新一次 → 重试一次 → 刷新失败进
  `reauthorization_required`）由 `nomifun-ai-agent/tests/mcp_oauth_e2e.rs`
  （引擎层，wiremock 全链路）与 `nomifun-mcp/tests/oauth_transport_integration.rs`
  （探针层）覆盖，证据见 `06-connector-oauth-security.md` §13（2026-09-01/02，含
  QQ Mail 真实服务闭环），未纳入本 live 脚本；live 脚本未做真实模型调用
  （探针即证明注入生效）。

### 5. 复跑

```bash
cargo build -p agent-store
cd web && AGENT_STORE_BIN=../target/debug/agent-store.exe bun scripts/sdk-live-oauth.ts
# OAUTH_KEEP_DATA=1 保留 data dir 供取证；默认关闭后自删。
```

回归：`cargo test -p nomifun-mcp`（251 + 集成全绿）、
`cargo test -p nomifun-ai-agent --test mcp_oauth_e2e`。

---

## 13. 附录 B · MCP OAuth 运行时闭环证据

> 本节由 `mcp-oauth-runtime-evidence.zh.md` 整体并入（2026-09-11）。定性同附录 A。


> 日期：2026-09-01（①②）；2026-09-02（③ 真实服务全链路）
> 目标：`docs/agent-store/` Agent Store 路线图中 Connector OAuth 运行时闭环（登录 → token 存储 → 请求时注入 → 工具调用成功；401 刷新 + 单次重试）
> 状态：①②③ 全部完成 —— ③ 以 QQ Mail MCP 真实服务完成端到端验证（见 §6），登录环节由人工浏览器授权完成

### 1. 实现范围

#### ① Token 注入（运行时传输层）

| 组件 | 位置 | 内容 |
|---|---|---|
| 引擎传输层 | `crates/agent/nomi-mcp/src/transport/streamable_http.rs`、`sse.rs` | 401 结构化为 `McpError::Unauthorized`；`McpTransport::update_auth_header()` 更新 remote 传输的 `Authorization` 头 |
| 引擎管理器 | `crates/agent/nomi-mcp/src/manager.rs` | `McpOAuthRefresher` trait；`connect_all_with_oauth` 记录 remote server URL；`request_server` 401 → 刷新一次 → 更新头 → **单次重试** |
| 引擎装配 | `crates/agent/nomi-agent/src/bootstrap.rs` | `AgentBootstrap::mcp_oauth_refresher` 穿线到 `McpManager` |
| 后端注入 | `crates/backend/nomifun-ai-agent/src/factory/mcp_oauth.rs` | `inject_oauth_bearer`：会话构建时按 URL 从加密 token 库取 token（过期自动刷新）注入 `Authorization: Bearer`（用户显式 Authorization 优先）；`NomiMcpOAuthRefresher` 复用 `McpOAuthService::refresh_access_token` |
| 工厂装配 | `nomifun-ai-agent/src/factory/nomi.rs`、`manager/nomi/agent.rs`、`nomifun-app/src/services.rs` | `AgentFactoryDeps.mcp_oauth_service` → `NomiHostWiring.mcp_oauth_refresher` → bootstrap |

stdio → env：连接器 transport 的 env 原样透传（stdio 无 URL，OAuth 不适用；env 型凭据由配置直通）。

#### OAuth 服务能力补齐（真实世界必需）

| 能力 | 位置 |
|---|---|
| RFC 9728 Protected Resource Metadata 发现（GitHub 等现代服务器） | `nomifun-mcp/src/oauth_service.rs::discover_endpoints` / `discover_protected_resource_metadata` |
| RFC 7591 动态客户端注册 | `register_client`（无注册端点/失败时回退内置 public client） |
| 预注册 client 通道 | `MCP_OAUTH_CLIENT_ID` / `MCP_OAUTH_CLIENT_SECRET` / `MCP_OAUTH_REDIRECT_URI`（env，参考 `mcp-client-oauth` 模式） |
| 测试接缝 | `McpOAuthService::new_with_browser_hook`（替代系统浏览器，驱动回调） |
| 可观测性 | `nomifun-app/src/app_server_catalog.rs::auth_start` 记录登录结果日志 |

### 2. 自动化端到端证据（核心闭环）

`crates/backend/nomifun-ai-agent/tests/mcp_oauth_e2e.rs` —— 本地完整 OAuth 保护 MCP 服务器（wiremock：RFC 9728 PRM、RFC 8414、RFC 7591、authorize/token、受保护 `tools/call`），**一次通过**：

```text
✅ login：RFC 9728 发现 → 动态注册 → PKCE → loopback 回调（浏览器钩子驱动）
✅ token 加密存储：check_oauth_status=authenticated；get_token 命中
✅ ① 请求时注入：inject_oauth_bearer → Authorization: Bearer e2e-access-token
✅ 运行时工具调用：McpManager 连接（带注入头）→ tools/list → tools/call → "pong"
✅ ② 服务端吊销 → 401 → refresh_access_token（refresh_token grant）→ 更新头 → 单次重试成功
✅ 刷新后 token 持久化
```

### 3. 单元测试证据

```text
nomi-mcp          122 passed（含 401 刷新+单次重试、无 refresher 时 401 原样透出）
nomi-agent        747 passed
nomifun-mcp       249 + 4 + 10 passed（含 RFC 9728 发现、动态注册错误路径、
                  pre-registered env 覆盖、resource_metadata 解析）
nomifun-app-server 47 passed（Skill/Connector 目录与能力协商）
factory mcp_oauth  2 passed；session snapshot 1 passed
```

### 4. 真实服务探测（GitHub Copilot MCP）

- `GET https://api.githubcopilot.com/mcp/` → 401 + `WWW-Authenticate: ... resource_metadata="https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/"`
- PRM → `authorization_servers: [https://github.com/login/oauth]`（scopes: repo/read:org/...）
- github.com 不发布 RFC 8414 元数据 → **不支持动态注册**（官方佐证：[claude-plugins-official#283](https://github.com/anthropics/claude-plugins-official/issues/283)、[claude-code#3433](https://github.com/anthropics/claude-code/issues/3433)）→ 需预注册 GitHub OAuth App client_id
- 后端日志实测（运行中服务）：`WARN ... OAuth dynamic client registration failed ... authorization server advertises no registration_endpoint` —— 证明 RFC 9728 发现链路在真实服务上生效
- 连接器状态机正确：未授权 → `authorization_required` / `not_authenticated`，工具列表为空（不伪报 connected）

### 5. 待人工输入（阻塞项，第 3 轮起）

真实 marketplace 服务的浏览器授权必须由人完成。可选：

1. 提供 GitHub OAuth App 的 `client_id`（Callback URL 填 `http://127.0.0.1:8765/callback`）；设置
   `MCP_OAUTH_CLIENT_ID` + `MCP_OAUTH_REDIRECT_URI=http://127.0.0.1:8765/callback` 后重启后端，触发
   `connector/auth/start` 并完成浏览器授权，即可验证真实 GitHub 工具调用；
2. 或指定一个支持动态注册/公开 client 的真实远程 MCP 服务 URL。

marketplace 150 个连接器均为平台 server-side 账号体系，浏览器授权环节不可跳过。

> 用户选择屏蔽 GitHub（需要预注册 client），改走 **QQ Mail MCP**（WorkBuddy 凭据中公开 client_id）并完成浏览器授权 —— 见 §6。

### 6. 真实服务端到端验证：QQ Mail MCP（2026-09-02，③ 完成）

#### 6.1 验证拓扑

| 项 | 值 |
|---|---|
| MCP 服务 | `https://api.mail.qq.com/mcp`（marketplace `qqmail` 连接器 `01a05fd5-de94-7ac2-8c93-ad9bd763d4a3`） |
| OAuth client | `MCP_OAUTH_CLIENT_ID=002e8cd14071aad2`（取自 WorkBuddy 凭据公开字段），redirect `http://127.0.0.1:8765/callback` |
| 后端 | `nomifun-web --port 8787 --api-only --insecure-no-auth --data-dir …\allo-web-real-backend` |
| 模型 | `C:\Users\15165\.agent-store\config.toml` 的 mimo provider（`https://api.xiaomimimo.com/v1`，`mimo-v2.5`，1M 上下文，无限流），经 `POST /api/providers` 注册 |

#### 6.2 链路证据（四步闭环）

**① 登录（浏览器授权，人工）**：`connector/auth/start` → 浏览器打开 QQ 授权页 → 用户授权 → 回调完成。
`Authorization` 前全链路命中：

- `connector/auth/status` → `{"state":"authenticated"}`（登录瞬间）与后端**重启之后**（跨进程持久化）均返回 `authenticated`

**② token 存储**：`oauth_tokens` 表（SQLite，`flowy-backend.db`）：

```text
oauth_tokens: server_url=https://api.mail.qq.com/mcp
  access_token=<156 字符> refresh_token=<120 字符> token_type=bearer
  expires_at=1788318257314 (≈+1h)  created/updated=1788314657314
mcp_servers: qqmail 行 enabled=1, transport=Http{url=https://api.mail.qq.com/mcp}
```

重启后端后 `auth-status` 仍为 `authenticated` —— 令牌跨进程持久化生效。

**③ 请求时注入（真实服务器对比实验）**：

| 实验 | 结果 |
|---|---|
| 无 token 直连 `initialize`（curl，任意协议版本） | HTTP 401 `{"code":40101,"message":"Bearer token is missing…"}` |
| `connector/test`（连接测试客户端，注入存储的 Bearer） | 通过 401 关卡 → 服务器返回 JSON-RPC 层响应 → `success: true` + **12 个真实工具**（GetMe/ListMessages/GetMessage/SendMessage/SearchMessages/…） |

**④ 工具调用成功（真实运行时 + 真实邮件数据）**：

- 创建 nomi 会话并绑定连接器：`extra.selected_mcp_server_ids=[01a05fd5-…]` → 会话 `extra.mcp_server_ids` 落库、`mcp_servers=["qqmail"]`
- 后端日志：`nomi_mcp: mcp server connected server=qqmail tools=12`（对接运行时 `McpManager`，注入头生效；此前无 token 时同一入口为 401）
- 模型（mimo-v2.5）经 `McpManager::request_server` 调用真实工具：

```json
{
  "name": "mcp__qqmail__ListMessages__ffj4jx2ty652yejx",
  "args": {"limit": 3, "dir": "inbox"},
  "status": "completed"
}
```

返回**真实邮件数据**（节选）：3 封收件箱邮件 —— `10000@qq.com`「"OpenClaw"已获得了你的QQ邮箱账号的访问权限」「"WorkBuddy"已获得了你的QQ邮箱账号的访问权限」、`informer@daily.dev`「Alioth, your personal update from daily.dev is ready」，收件人均为 `1516544795@qq.com`，含分页游标 `next_cursor` 与 API 自述 `_hints`。最终模型输出中文摘要（发件人/主题/UTC+8 时间）。

#### 6.3 过程中发现并修复的真实兼容性问题

QQ Mail 网关拒绝旧协议版本：`initialize error: Unsupported protocol version: 2024-11-05 (code 600001)`。

- 运行时传输层（`nomi-mcp` `remote_peer.rs`）已使用 `CLIENT_PROTOCOL_VERSION = 2025-11-25`；
- 连接测试客户端（`nomifun-mcp` `connection_test/protocol.rs`）仍硬编码 `2024-11-05` → **对齐为 `2025-11-25`**，注释注明与运行时保持同步；
- 修复后 `connector/test` 立即成功（§6.2 ③）。

#### 6.4 回归

- `nomifun-mcp` 全量测试（lib + 集成）在协议常量修改后重跑：**全部通过**；
- 修改仅涉及连接测试的 `initialize` 协议版本常量，运行时行为不变。

#### 6.5 遗留说明

- 会话层面绑定连接器时需显式指定模型（`model:{provider_id,model}`），否则报 `Nomi conversation has no provider/model configured`；
- 模型工具发现行为：语言模型倾向先用 `ToolSearch` 检索 deferred 工具（MCP 工具是直接注册工具，不在 deferred registry），提示词明示「直接调用工具列表中的邮件工具」后一次成功 —— 属模型提示工程问题，非传输层缺陷。

---

## 14. 附录 C · MCP OAuth 动态客户端注册技术方案

> 本节由 `nomifun-mcp-oauth-dynamic-client-registration-design.md` 整体并入（2026-09-11）。这是**设计记录**（含已实现部分），非契约正文。


> 状态：✅ 已实现（2026-09-02，见「§12 实现记录」）
> 范围：Agent Store 出站远程 MCP Connector 的 OAuth 登录与凭据生命周期
> 关联：[Connector、OAuth 与安全模型](06-connector-oauth-security.md)；MCP OAuth 运行时闭环验证证据见本文件 §13。

### 1. 背景与目标

Agent Store 作为 MCP Client 连接远程 Streamable HTTP MCP Server 时，需要支持两类 OAuth 服务：

1. 服务方预注册客户端，产品侧提供 `client_id`，可选 `client_secret`；
2. 服务方发布 RFC 8414 metadata 和 RFC 7591 `registration_endpoint`，客户端可动态注册并获取 `client_id`。

现有 `nomifun-mcp::McpOAuthService` 已具备 Metadata discovery、PKCE、loopback callback、authorization code 换 token、token 持久化、refresh token 和初步动态注册。但是动态注册产生的 client identity 仅保留在进程内存中，重启后的 refresh 可能错误地使用内置默认 ID；RFC 9728 discovery 也只对 endpoint 发 GET，不能兼容仅对 MCP `initialize` POST 返回 OAuth challenge 的服务。

本方案目标是将动态注册从一次性登录能力升级为可持续运行的 Connector OAuth 能力：

```text
发现 OAuth Server
  → 选择预注册或动态注册 client identity
  → PKCE 授权
  → Token 与注册信息安全持久化
  → 运行时注入 Bearer Token
  → 401 时以原 client identity 刷新并单次重试
```

本方案不实现自定义 URI Scheme、公网 relay、OAuth over stdio、非标准 OAuth endpoint，也不要求针对某个厂商写特例。

### 2. 当前实现与问题

#### 2.1 已有职责边界

```text
nomifun-mcp
  discovery、动态注册、PKCE、callback、token exchange、token refresh、OAuth API

nomi-mcp
  stdio/SSE/Streamable HTTP transport、Bearer 注入、401 单次刷新重试入口

nomifun-ai-agent
  取安全存储的 token 注入运行时，并将 refresh 服务适配为 McpOAuthRefresher
```

该边界保持不变。运行时 transport 不持有 OAuth client secret、refresh token 或浏览器授权状态。

#### 2.2 待解决问题

1. `RegisteredClient` 仅存于 `Arc<Mutex<HashMap<...>>>`；应用重启后丢失。
2. refresh token 的 token endpoint 请求可能退回 `DEFAULT_CLIENT_ID = "nomifun"`，与原始 token 绑定的 client identity 不一致。
3. 动态注册失败后继续使用内置 ID 会产生不可诊断的 `invalid_client`，应返回明确的“需要预注册 client”或“注册失败”。
4. RFC 9728 仅以 GET 探测时，无法发现仅在未经认证的 `initialize` POST 上返回 `WWW-Authenticate` 的 MCP 服务。
5. OAuth 凭据当前以 server URL 为主要键，不足以隔离不同 OAuth issuer、resource、redirect URI 或 client identity。

### 3. 设计原则

- 入站 Agent Store 身份认证与出站 Connector OAuth 凭据完全隔离。
- 真实 token、client secret、registration access token 仅存安全凭据存储；不进入普通配置、日志、事件、Renderer 或公共 API 响应。
- 动态注册 client identity 的复用范围由 `mcp_server_url + resource + issuer + redirect_uri` 共同确定。
- 预注册 client 显式优先于动态注册；缺少两者之一时授权必须失败，不构造虚假的默认 client identity。
- 登录后 `connected` 必须由实际初始化/工具发现 probe 得出，`authenticated` 仅表示 OAuth token 已保存。
- 401 刷新最多一次并只重试原请求一次，避免循环。

### 4. 领域模型与持久化

#### 4.1 OAuthClientRegistration

新增持久化 registration 记录。推荐独立于 token 表，避免把 client identity 误建模成用户 token 的附属字段。

```text
OAuthClientRegistration
├── id
├── mcp_server_url             # 规范化后的 MCP endpoint
├── resource_identifier        # RFC 9728 resource
├── authorization_server_issuer
├── redirect_uri
├── registration_mode          # dynamic | pre_registered
├── client_id
├── client_secret_ref          # 可选，安全存储引用
├── registration_access_token_ref # 可选，安全存储引用
├── client_id_issued_at        # 可选
├── client_secret_expires_at   # 可选
├── registration_client_uri    # 可选
├── created_at
└── updated_at
```

唯一性约束：

```text
(mcp_server_url, resource_identifier, authorization_server_issuer, redirect_uri)
```

`client_secret_ref` 和 `registration_access_token_ref` 指向已有安全凭据机制；不得将敏感字段落入 SQLite 明文列。

#### 4.2 OAuthTokenCredential

现有 OAuth token 数据扩展或通过外键关联 registration：

```text
OAuthTokenCredential
├── registration_id
├── principal_id               # 当前 V1 若为本机单用户也应预留
├── access_token_ref
├── refresh_token_ref
├── token_type
├── scope
├── expires_at
├── created_at
└── updated_at
```

Token 的查询不得只按 URL；至少以 registration identity 和 principal 绑定。不同用户、不同 resource、不同 issuer 或不同 client 不得复用 token。

#### 4.3 迁移与兼容

- 为历史 `oauth_tokens` 创建兼容迁移：已有 token 在无法可靠回推 registration identity 时标记为 `requires_reauthorization`，而不是猜测 client ID。
- 迁移必须可重复执行，repository 对缺失旧数据返回可识别状态。
- 预注册 client 的 client ID 可以来自环境变量或受控 Connector credential；环境变量仅作为当前开发兼容入口，生产产品优先采用凭据绑定。

### 5. OAuth Discovery

#### 5.1 Discovery 顺序

给定远程 MCP URL，按下列顺序发现 Authorization Server：

1. 请求 `<server-url>/.well-known/oauth-authorization-server`；
2. 请求 `<server-url>/.well-known/openid-configuration`；
3. 请求 `<origin>/.well-known/oauth-authorization-server`；
4. 请求 `<origin>/.well-known/openid-configuration`；
5. 未找到时，对 MCP endpoint 发送未认证的 Streamable HTTP `initialize` POST；
6. 解析 401 中 `WWW-Authenticate` 的 `resource_metadata`；
7. 拉取 RFC 9728 Protected Resource Metadata，读取 `resource` 和第一个支持的 `authorization_servers` issuer；
8. 拉取 issuer 的 RFC 8414 或 OIDC metadata。

`initialize` 探测请求：

```http
POST <server-url>
Content-Type: application/json
Accept: application/json, text/event-stream
```

```json
{
  "jsonrpc": "2.0",
  "id": "oauth-discovery",
  "method": "initialize",
  "params": {
    "protocolVersion": "<当前 nomi-mcp 协议常量>",
    "capabilities": {},
    "clientInfo": {
      "name": "Nomifun MCP Client",
      "version": "<应用版本>"
    }
  }
}
```

不得在 OAuth discovery 中复制协议版本常量；应复用或公开 `nomi-mcp` 使用的当前 MCP protocol version。

#### 5.2 Discovery 输出

发现过程输出内部 `ResolvedOAuthServer`：

```text
ResolvedOAuthServer
├── resource_identifier
├── authorization_server_issuer
├── authorization_endpoint
├── token_endpoint
├── registration_endpoint? 
├── supported_pkce_methods
└── scopes_supported
```

最低要求：`authorization_endpoint`、`token_endpoint`。若不含 `registration_endpoint` 且没有预注册 client，返回 `pre_registered_client_required`。

### 6. Client Identity 决策与注册

#### 6.1 决策顺序

```text
存在有效的预注册 client 配置
  → 使用预注册 client，不调用动态注册

否则存在同一 identity key 的有效持久化动态 registration
  → 复用该 registration

否则 Authorization Server 发布 registration_endpoint
  → RFC 7591 动态注册并安全持久化 registration

否则
  → 返回 pre_registered_client_required
```

不得使用 `DEFAULT_CLIENT_ID` 作为通用回退。仅当项目将某个 ID 建模为特定 OAuth provider 已登记、可审计的 connector credential 时才能使用，且不得冒充通用默认值。

#### 6.2 RFC 7591 请求

动态注册使用 public native client 和 PKCE：

```json
{
  "client_name": "Nomifun MCP Client",
  "redirect_uris": ["<实际 callback URI>"],
  "grant_types": ["authorization_code", "refresh_token"],
  "response_types": ["code"],
  "token_endpoint_auth_method": "none"
}
```

响应必须包含 `client_id`。可选字段 `client_secret`、`client_id_issued_at`、`client_secret_expires_at`、`registration_access_token`、`registration_client_uri` 按数据模型保存。

注册失败的错误需区分：

```text
registration_not_supported
redirect_uri_not_allowed
dynamic_registration_failed
invalid_registration_response
pre_registered_client_required
```

错误对 UI 可读，但不得含 token、authorization code 或完整 authorization URL。

### 7. 登录、Callback 与 Token 生命周期

#### 7.1 Callback

默认使用 loopback listener：

```text
http://127.0.0.1:<ephemeral-port>/callback
```

支持固定 loopback callback，例如：

```text
MCP_OAUTH_REDIRECT_URI=http://127.0.0.1:8989/oauth/callback
```

固定 URI 时：

- 仅接受 `http://127.0.0.1` 或 `http://localhost`；
- listener 绑定 URI 指定的 host 与 port；
- HTTP 请求 path 必须等于 URI path；
- 注册请求、authorize request、token exchange 使用同一个完整 redirect URI；
- 非 loopback、缺少端口或 path 不匹配返回 `unsupported_auth` 或参数错误。

生成高熵 `state` 与 PKCE S256 verifier/challenge。callback 仅消费一次；校验 `state` 后才交换 code。浏览器成功页不得回显 code、state 或 token。

#### 7.2 Token Exchange 与 Refresh

- Authorization code exchange 使用产生授权 URL 的同一个 registration record。
- refresh token 请求通过 `registration_id` 查询原 client identity；不得重新选择默认或其它 client。
- token 过期前按现有安全窗口刷新。
- refresh token 无效、client 被撤销或 secret 到期时，清除失效 token 并返回 `reauthorization_required`；保留 registration 是否可复用由服务端错误和期限决定。
- MCP transport 收到 401 时，调用后端 refresher、更新 Bearer header、重试一次；第二次 401 或 refresh 失败即结束请求。

### 8. API 与状态机

保留现有登录接口形态，但在响应中使用不含敏感值的状态：

```text
not_authenticated
authorization_pending
authenticated
connected
reauthorization_required
pre_registered_client_required
unsupported_auth
error
```

状态含义：

```text
authenticated = 凭据已成功存储
connected     = 使用该凭据完成 initialize / tools/list probe
```

Connector OAuth API 不向 Renderer 返回 client secret、registration access token、access token、refresh token、code verifier、authorization code 或完整回调 URL。授权 URL 仅经受控的系统浏览器打开机制传递。

### 9. 实现位置

| 模块 | 变更 |
|---|---|
| `crates/backend/nomifun-mcp/src/oauth_service.rs` | discovery POST fallback、registration 决策、callback path 校验、使用持久化 registration 的 exchange/refresh |
| `crates/backend/nomifun-db` | registration migration、repository、加密敏感值引用、token 与 registration 关联 |
| `crates/backend/nomifun-mcp/src/routes.rs` 与 API types | 返回结构化授权状态与可处理错误 |
| `crates/backend/nomifun-ai-agent/src/factory/mcp_oauth.rs` | 以 registration identity 获取/刷新 token，保持 `McpOAuthRefresher` 适配 |
| `crates/agent/nomi-mcp` | 只在必要时公开共享 MCP protocol version；保持一次刷新一次重试 |

### 10. 测试与验收

使用 mock OAuth Server 和 mock MCP Server，不在自动化测试中调用真实服务。

1. metadata 无 `registration_endpoint` 且无预注册 client：返回 `pre_registered_client_required`，不发送 token request。
2. metadata 有 registration endpoint：验证 RFC 7591 payload，注册响应 client ID 被持久化，authorize/token exchange 均使用该 ID 与 PKCE verifier。
3. 应用重建后：读取同一 registration；refresh token 请求使用原动态 client ID；不重新注册且不使用默认 ID。
4. 配置预注册 client：不发送注册请求，使用其 client ID、可选 secret 与固定 redirect URI。
5. 动态注册拒绝 redirect URI：返回 `redirect_uri_not_allowed`，不继续构造授权 URL。
6. MCP endpoint GET 返回 405、initialize POST 返回含 `resource_metadata` 的 401：成功完成 RFC 9728 discovery。
7. callback 覆盖正确 state、错误 state、缺少 code、错误 path、重复 callback。
8. 运行时调用覆盖 401 → refresh 成功 → 单次重试成功；refresh 失败和第二次 401 均不再重试。
9. 扫描日志、公共响应和普通配置，确认不含 token、secret、authorization code、verifier 或 registration access token。

完成实现后至少执行：

```bash
cargo fmt --check
cargo test -p nomifun-mcp
cargo test -p nomi-mcp
cargo clippy -p nomifun-mcp -p nomi-mcp -- -D warnings
```

实际 package 名或 workspace 命令有差异时，以 workspace manifest 为准并报告实际命令与结果。

### 11. 交付边界

本方案的完成条件是：动态注册服务可在首次登录后持久化 client identity，应用重启后仍使用同一 identity 刷新 token，并能通过真实 MCP `initialize` POST 的 OAuth challenge 完成标准发现。

厂商是否接受具体 loopback URI、是否支持动态注册以及实际 scope 授权，属于上游 OAuth Server 能力。接入某个真实服务前仍需独立完成浏览器授权、`tools/list` 与 refresh 验证，不能由模拟测试替代。

### 12. 实现记录（2026-09-02）

#### 交付内容

| 模块 | 实现 |
|---|---|
| `nomifun-db` | migration `052_oauth_client_registrations.sql`（registrations 表 + `oauth_tokens.registration_id/principal_id` 逻辑关联列 + 索引）；`OAuthClientRegistrationRow`、`IOAuthClientRegistrationRepository`、`SqliteOAuthClientRegistrationRepository`（身份键 upsert/查询/删除 + 单测）；`OAuthTokenRow` 扩展、`IOAuthTokenRepository::get_by_registration`、`UpsertOAuthTokenParams` 增 `registration_id/principal_id`；v3 schema 注册（`PRODUCT_TABLES`/`NON_REFERENCE_ID_COLUMNS`） |
| `nomifun-mcp` | `MCP_PROTOCOL_VERSION` 常量（与 `nomi-mcp::MCP_PROTOCOL_VERSION` 一致，由 `nomifun-ai-agent` 断言）；`discover_endpoints` → `ResolvedOAuthServer`（RFC 9728 `resource` + issuer）；**未认证 initialize POST 回退发现**；`resolve_client_identity`（预注册 > 持久化动态注册 > RFC 7591 > `pre_registered_client_required`）；`register_client` 持久化 + 错误分类（`redirect_uri_not_allowed`/`dynamic_registration_failed`/`invalid_registration_response`）；exchange/refresh 绑定原 registration（refresh 不再回退默认 client）；固定 loopback redirect 校验（host/port/path）；callback path 校验；`McpError` 结构化错误码 + `oauth_error_code()`；服务注解 |
| `nomifun-api-types` | `OAuthStatusResponse.state`（§8 状态机枚举）保留 `authenticated` 兼容；`OAuthLoginResponse.error_code` |
| `nomifun-app` | `services.rs` 注入 `SqliteOAuthClientRegistrationRepository` |
| `nomifun-ai-agent` | `MCP_PROTOCOL_VERSION` 跨 crate 一致性断言 |

#### 验收结果（mock OAuth + MCP Server，无真实服务）

`crates/backend/nomifun-mcp/tests/dynamic_registration_integration.rs` —— **9/9 通过**：

```text
✅ 1 no_registration_endpoint_without_pre_registered_client_fails_loudly
      （pre_registered_client_required；无 register/token/authorize 请求）
✅ 2 dynamic_registration_persists_and_exchange_uses_registered_id
      （RFC 7591 payload；authorize/exchange 以注册 client 认证 + PKCE verifier）
✅ 3 rebuilt_service_reuses_registration_for_refresh
      （重建服务不重新注册；refresh 以原动态 client 身份 Basic 认证）
✅ 4 pre_registered_client_skips_dynamic_registration（env 通道；零注册请求）
✅ 5 registration_rejects_redirect_uri（redirect_uri_not_allowed；authorize 未发起）
✅ 6 discovery_via_initialize_post_when_get_rejected（GET 405 → POST 401 发现）
✅ 7 callback_state_mismatch_is_rejected / callback_path_mismatch_is_rejected
✅ 9 login_errors_carry_no_sensitive_material（不含 code/state/verifier/授权 URL）
#8 401 → refresh → 单次重试由 nomifun-ai-agent/tests/mcp_oauth_e2e.rs 覆盖
```

#### 全量门禁（实际命令与结果）

```text
cargo fmt -p nomifun-mcp -p nomi-mcp -p nomifun-db -p nomifun-ai-agent --check   ✅ 无差异
cargo test -p nomifun-db --lib                                                 ✅ 431 passed
cargo test -p nomifun-mcp                                                      ✅ 251 lib + 集成全过
cargo test -p nomi-mcp --lib                                                   ✅ 122 passed
cargo test -p nomifun-ai-agent --test mcp_oauth_e2e                            ✅ 全链路通过
cargo clippy -p nomifun-mcp --all-targets                                      ✅ 本方案文件零警告
cargo clippy -p nomi-mcp --all-targets                                         ✅ 本方案文件零警告
```

说明：`clippy -D warnings` 在依赖 crate 上不干净属于仓库既有基线（nomi-coding、
nomi-agent-trace、nomi-process-runtime、nomifun-common 等存量 warning，与本次改动
无关）；本次新增/修改文件（oauth_service、dynamic_registration_integration、
remote_peer 常量导出、db repository/模型等）在 `--all-targets` 下零警告。

#### 关键行为变更（与旧实现的差异）

- **不再回退 `DEFAULT_CLIENT_ID`**：无预注册 client 且服务方无 `registration_endpoint` 时登录返回
  `pre_registered_client_required`（原先默默使用内置 `nomifun` id）；
- **refresh 绑定原 identity**：token 无 `registration_id` 且无 env 预注册 client 时，
  refresh 返回 `reauthorization_required` 而非猜测身份（兼容通道：env 预注册仍可用）；
- **callback 严格化**：固定 `MCP_OAUTH_REDIRECT_URI` 只接受 loopback + 显式端口 + 明确 path，
  回调请求 path 必须精确匹配注册的 redirect path，否则 `unsupported_auth` 且不消耗登录状态；
- `login()` 对发现/注册失败返回结构化 `OAuthLoginResponse`（`success=false` +
  `error_code`），不再以 `Err` 直接抛出。
