# MCP OAuth 运行时闭环 — 验证证据

> 日期：2026-09-01（①②）；2026-09-02（③ 真实服务全链路）
> 目标：`docs/agent-store/` Agent Store 路线图中 Connector OAuth 运行时闭环（登录 → token 存储 → 请求时注入 → 工具调用成功；401 刷新 + 单次重试）
> 状态：①②③ 全部完成 —— ③ 以 QQ Mail MCP 真实服务完成端到端验证（见 §6），登录环节由人工浏览器授权完成

## 1. 实现范围

### ① Token 注入（运行时传输层）

| 组件 | 位置 | 内容 |
|---|---|---|
| 引擎传输层 | `crates/agent/nomi-mcp/src/transport/streamable_http.rs`、`sse.rs` | 401 结构化为 `McpError::Unauthorized`；`McpTransport::update_auth_header()` 更新 remote 传输的 `Authorization` 头 |
| 引擎管理器 | `crates/agent/nomi-mcp/src/manager.rs` | `McpOAuthRefresher` trait；`connect_all_with_oauth` 记录 remote server URL；`request_server` 401 → 刷新一次 → 更新头 → **单次重试** |
| 引擎装配 | `crates/agent/nomi-agent/src/bootstrap.rs` | `AgentBootstrap::mcp_oauth_refresher` 穿线到 `McpManager` |
| 后端注入 | `crates/backend/nomifun-ai-agent/src/factory/mcp_oauth.rs` | `inject_oauth_bearer`：会话构建时按 URL 从加密 token 库取 token（过期自动刷新）注入 `Authorization: Bearer`（用户显式 Authorization 优先）；`NomiMcpOAuthRefresher` 复用 `McpOAuthService::refresh_access_token` |
| 工厂装配 | `nomifun-ai-agent/src/factory/nomi.rs`、`manager/nomi/agent.rs`、`nomifun-app/src/services.rs` | `AgentFactoryDeps.mcp_oauth_service` → `NomiHostWiring.mcp_oauth_refresher` → bootstrap |

stdio → env：连接器 transport 的 env 原样透传（stdio 无 URL，OAuth 不适用；env 型凭据由配置直通）。

### OAuth 服务能力补齐（真实世界必需）

| 能力 | 位置 |
|---|---|
| RFC 9728 Protected Resource Metadata 发现（GitHub 等现代服务器） | `nomifun-mcp/src/oauth_service.rs::discover_endpoints` / `discover_protected_resource_metadata` |
| RFC 7591 动态客户端注册 | `register_client`（无注册端点/失败时回退内置 public client） |
| 预注册 client 通道 | `MCP_OAUTH_CLIENT_ID` / `MCP_OAUTH_CLIENT_SECRET` / `MCP_OAUTH_REDIRECT_URI`（env，参考 `mcp-client-oauth` 模式） |
| 测试接缝 | `McpOAuthService::new_with_browser_hook`（替代系统浏览器，驱动回调） |
| 可观测性 | `nomifun-app/src/app_server_catalog.rs::auth_start` 记录登录结果日志 |

## 2. 自动化端到端证据（核心闭环）

`crates/backend/nomifun-ai-agent/tests/mcp_oauth_e2e.rs` —— 本地完整 OAuth 保护 MCP 服务器（wiremock：RFC 9728 PRM、RFC 8414、RFC 7591、authorize/token、受保护 `tools/call`），**一次通过**：

```text
✅ login：RFC 9728 发现 → 动态注册 → PKCE → loopback 回调（浏览器钩子驱动）
✅ token 加密存储：check_oauth_status=authenticated；get_token 命中
✅ ① 请求时注入：inject_oauth_bearer → Authorization: Bearer e2e-access-token
✅ 运行时工具调用：McpManager 连接（带注入头）→ tools/list → tools/call → "pong"
✅ ② 服务端吊销 → 401 → refresh_access_token（refresh_token grant）→ 更新头 → 单次重试成功
✅ 刷新后 token 持久化
```

## 3. 单元测试证据

```text
nomi-mcp          122 passed（含 401 刷新+单次重试、无 refresher 时 401 原样透出）
nomi-agent        747 passed
nomifun-mcp       249 + 4 + 10 passed（含 RFC 9728 发现、动态注册错误路径、
                  pre-registered env 覆盖、resource_metadata 解析）
nomifun-app-server 47 passed（Skill/Connector 目录与能力协商）
factory mcp_oauth  2 passed；session snapshot 1 passed
```

## 4. 真实服务探测（GitHub Copilot MCP）

- `GET https://api.githubcopilot.com/mcp/` → 401 + `WWW-Authenticate: ... resource_metadata="https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/"`
- PRM → `authorization_servers: [https://github.com/login/oauth]`（scopes: repo/read:org/...）
- github.com 不发布 RFC 8414 元数据 → **不支持动态注册**（官方佐证：[claude-plugins-official#283](https://github.com/anthropics/claude-plugins-official/issues/283)、[claude-code#3433](https://github.com/anthropics/claude-code/issues/3433)）→ 需预注册 GitHub OAuth App client_id
- 后端日志实测（运行中服务）：`WARN ... OAuth dynamic client registration failed ... authorization server advertises no registration_endpoint` —— 证明 RFC 9728 发现链路在真实服务上生效
- 连接器状态机正确：未授权 → `authorization_required` / `not_authenticated`，工具列表为空（不伪报 connected）

## 5. 待人工输入（阻塞项，第 3 轮起）

真实 marketplace 服务的浏览器授权必须由人完成。可选：

1. 提供 GitHub OAuth App 的 `client_id`（Callback URL 填 `http://127.0.0.1:8765/callback`）；设置
   `MCP_OAUTH_CLIENT_ID` + `MCP_OAUTH_REDIRECT_URI=http://127.0.0.1:8765/callback` 后重启后端，触发
   `connector/auth/start` 并完成浏览器授权，即可验证真实 GitHub 工具调用；
2. 或指定一个支持动态注册/公开 client 的真实远程 MCP 服务 URL。

marketplace 150 个连接器均为平台 server-side 账号体系，浏览器授权环节不可跳过。

> 用户选择屏蔽 GitHub（需要预注册 client），改走 **QQ Mail MCP**（WorkBuddy 凭据中公开 client_id）并完成浏览器授权 —— 见 §6。

## 6. 真实服务端到端验证：QQ Mail MCP（2026-09-02，③ 完成）

### 6.1 验证拓扑

| 项 | 值 |
|---|---|
| MCP 服务 | `https://api.mail.qq.com/mcp`（marketplace `qqmail` 连接器 `01a05fd5-de94-7ac2-8c93-ad9bd763d4a3`） |
| OAuth client | `MCP_OAUTH_CLIENT_ID=002e8cd14071aad2`（取自 WorkBuddy 凭据公开字段），redirect `http://127.0.0.1:8765/callback` |
| 后端 | `nomifun-web --port 8787 --api-only --insecure-no-auth --data-dir …\allo-web-real-backend` |
| 模型 | `C:\Users\15165\.agent-store\config.toml` 的 mimo provider（`https://api.xiaomimimo.com/v1`，`mimo-v2.5`，1M 上下文，无限流），经 `POST /api/providers` 注册 |

### 6.2 链路证据（四步闭环）

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

### 6.3 过程中发现并修复的真实兼容性问题

QQ Mail 网关拒绝旧协议版本：`initialize error: Unsupported protocol version: 2024-11-05 (code 600001)`。

- 运行时传输层（`nomi-mcp` `remote_peer.rs`）已使用 `CLIENT_PROTOCOL_VERSION = 2025-11-25`；
- 连接测试客户端（`nomifun-mcp` `connection_test/protocol.rs`）仍硬编码 `2024-11-05` → **对齐为 `2025-11-25`**，注释注明与运行时保持同步；
- 修复后 `connector/test` 立即成功（§6.2 ③）。

### 6.4 回归

- `nomifun-mcp` 全量测试（lib + 集成）在协议常量修改后重跑：**全部通过**；
- 修改仅涉及连接测试的 `initialize` 协议版本常量，运行时行为不变。

### 6.5 遗留说明

- 会话层面绑定连接器时需显式指定模型（`model:{provider_id,model}`），否则报 `Nomi conversation has no provider/model configured`；
- 模型工具发现行为：语言模型倾向先用 `ToolSearch` 检索 deferred 工具（MCP 工具是直接注册工具，不在 deferred registry），提示词明示「直接调用工具列表中的邮件工具」后一次成功 —— 属模型提示工程问题，非传输层缺陷。
