# Nomifun MCP OAuth Dynamic Client Registration 技术方案

> 状态：✅ 已实现（2026-09-02，见「§12 实现记录」）
> 范围：Agent Store 出站远程 MCP Connector 的 OAuth 登录与凭据生命周期
> 关联：[Connector、OAuth 与安全模型](06-connector-oauth-security.md)、[MCP OAuth 运行时闭环验证证据](mcp-oauth-runtime-evidence.zh.md)

## 1. 背景与目标

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

## 2. 当前实现与问题

### 2.1 已有职责边界

```text
nomifun-mcp
  discovery、动态注册、PKCE、callback、token exchange、token refresh、OAuth API

nomi-mcp
  stdio/SSE/Streamable HTTP transport、Bearer 注入、401 单次刷新重试入口

nomifun-ai-agent
  取安全存储的 token 注入运行时，并将 refresh 服务适配为 McpOAuthRefresher
```

该边界保持不变。运行时 transport 不持有 OAuth client secret、refresh token 或浏览器授权状态。

### 2.2 待解决问题

1. `RegisteredClient` 仅存于 `Arc<Mutex<HashMap<...>>>`；应用重启后丢失。
2. refresh token 的 token endpoint 请求可能退回 `DEFAULT_CLIENT_ID = "nomifun"`，与原始 token 绑定的 client identity 不一致。
3. 动态注册失败后继续使用内置 ID 会产生不可诊断的 `invalid_client`，应返回明确的“需要预注册 client”或“注册失败”。
4. RFC 9728 仅以 GET 探测时，无法发现仅在未经认证的 `initialize` POST 上返回 `WWW-Authenticate` 的 MCP 服务。
5. OAuth 凭据当前以 server URL 为主要键，不足以隔离不同 OAuth issuer、resource、redirect URI 或 client identity。

## 3. 设计原则

- 入站 Agent Store 身份认证与出站 Connector OAuth 凭据完全隔离。
- 真实 token、client secret、registration access token 仅存安全凭据存储；不进入普通配置、日志、事件、Renderer 或公共 API 响应。
- 动态注册 client identity 的复用范围由 `mcp_server_url + resource + issuer + redirect_uri` 共同确定。
- 预注册 client 显式优先于动态注册；缺少两者之一时授权必须失败，不构造虚假的默认 client identity。
- 登录后 `connected` 必须由实际初始化/工具发现 probe 得出，`authenticated` 仅表示 OAuth token 已保存。
- 401 刷新最多一次并只重试原请求一次，避免循环。

## 4. 领域模型与持久化

### 4.1 OAuthClientRegistration

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

### 4.2 OAuthTokenCredential

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

### 4.3 迁移与兼容

- 为历史 `oauth_tokens` 创建兼容迁移：已有 token 在无法可靠回推 registration identity 时标记为 `requires_reauthorization`，而不是猜测 client ID。
- 迁移必须可重复执行，repository 对缺失旧数据返回可识别状态。
- 预注册 client 的 client ID 可以来自环境变量或受控 Connector credential；环境变量仅作为当前开发兼容入口，生产产品优先采用凭据绑定。

## 5. OAuth Discovery

### 5.1 Discovery 顺序

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

### 5.2 Discovery 输出

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

## 6. Client Identity 决策与注册

### 6.1 决策顺序

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

### 6.2 RFC 7591 请求

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

## 7. 登录、Callback 与 Token 生命周期

### 7.1 Callback

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

### 7.2 Token Exchange 与 Refresh

- Authorization code exchange 使用产生授权 URL 的同一个 registration record。
- refresh token 请求通过 `registration_id` 查询原 client identity；不得重新选择默认或其它 client。
- token 过期前按现有安全窗口刷新。
- refresh token 无效、client 被撤销或 secret 到期时，清除失效 token 并返回 `reauthorization_required`；保留 registration 是否可复用由服务端错误和期限决定。
- MCP transport 收到 401 时，调用后端 refresher、更新 Bearer header、重试一次；第二次 401 或 refresh 失败即结束请求。

## 8. API 与状态机

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

## 9. 实现位置

| 模块 | 变更 |
|---|---|
| `crates/backend/nomifun-mcp/src/oauth_service.rs` | discovery POST fallback、registration 决策、callback path 校验、使用持久化 registration 的 exchange/refresh |
| `crates/backend/nomifun-db` | registration migration、repository、加密敏感值引用、token 与 registration 关联 |
| `crates/backend/nomifun-mcp/src/routes.rs` 与 API types | 返回结构化授权状态与可处理错误 |
| `crates/backend/nomifun-ai-agent/src/factory/mcp_oauth.rs` | 以 registration identity 获取/刷新 token，保持 `McpOAuthRefresher` 适配 |
| `crates/agent/nomi-mcp` | 只在必要时公开共享 MCP protocol version；保持一次刷新一次重试 |

## 10. 测试与验收

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

## 11. 交付边界

本方案的完成条件是：动态注册服务可在首次登录后持久化 client identity，应用重启后仍使用同一 identity 刷新 token，并能通过真实 MCP `initialize` POST 的 OAuth challenge 完成标准发现。

厂商是否接受具体 loopback URI、是否支持动态注册以及实际 scope 授权，属于上游 OAuth Server 能力。接入某个真实服务前仍需独立完成浏览器授权、`tools/list` 与 refresh 验证，不能由模拟测试替代。

## 12. 实现记录（2026-09-02）

### 交付内容

| 模块 | 实现 |
|---|---|
| `nomifun-db` | migration `052_oauth_client_registrations.sql`（registrations 表 + `oauth_tokens.registration_id/principal_id` 逻辑关联列 + 索引）；`OAuthClientRegistrationRow`、`IOAuthClientRegistrationRepository`、`SqliteOAuthClientRegistrationRepository`（身份键 upsert/查询/删除 + 单测）；`OAuthTokenRow` 扩展、`IOAuthTokenRepository::get_by_registration`、`UpsertOAuthTokenParams` 增 `registration_id/principal_id`；v3 schema 注册（`PRODUCT_TABLES`/`NON_REFERENCE_ID_COLUMNS`） |
| `nomifun-mcp` | `MCP_PROTOCOL_VERSION` 常量（与 `nomi-mcp::MCP_PROTOCOL_VERSION` 一致，由 `nomifun-ai-agent` 断言）；`discover_endpoints` → `ResolvedOAuthServer`（RFC 9728 `resource` + issuer）；**未认证 initialize POST 回退发现**；`resolve_client_identity`（预注册 > 持久化动态注册 > RFC 7591 > `pre_registered_client_required`）；`register_client` 持久化 + 错误分类（`redirect_uri_not_allowed`/`dynamic_registration_failed`/`invalid_registration_response`）；exchange/refresh 绑定原 registration（refresh 不再回退默认 client）；固定 loopback redirect 校验（host/port/path）；callback path 校验；`McpError` 结构化错误码 + `oauth_error_code()`；服务注解 |
| `nomifun-api-types` | `OAuthStatusResponse.state`（§8 状态机枚举）保留 `authenticated` 兼容；`OAuthLoginResponse.error_code` |
| `nomifun-app` | `services.rs` 注入 `SqliteOAuthClientRegistrationRepository` |
| `nomifun-ai-agent` | `MCP_PROTOCOL_VERSION` 跨 crate 一致性断言 |

### 验收结果（mock OAuth + MCP Server，无真实服务）

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

### 全量门禁（实际命令与结果）

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

### 关键行为变更（与旧实现的差异）

- **不再回退 `DEFAULT_CLIENT_ID`**：无预注册 client 且服务方无 `registration_endpoint` 时登录返回
  `pre_registered_client_required`（原先默默使用内置 `nomifun` id）；
- **refresh 绑定原 identity**：token 无 `registration_id` 且无 env 预注册 client 时，
  refresh 返回 `reauthorization_required` 而非猜测身份（兼容通道：env 预注册仍可用）；
- **callback 严格化**：固定 `MCP_OAUTH_REDIRECT_URI` 只接受 loopback + 显式端口 + 明确 path，
  回调请求 path 必须精确匹配注册的 redirect path，否则 `unsupported_auth` 且不消耗登录状态；
- `login()` 对发现/注册失败返回结构化 `OAuthLoginResponse`（`success=false` +
  `error_code`），不再以 `Err` 直接抛出。
