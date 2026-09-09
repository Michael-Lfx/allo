# Agent Store OAuth 运行时证据（TC-OAUTH-001/002/004）

> 日期：2026-09-09
> 脚本：`web/scripts/sdk-live-oauth.ts`（可重复门禁脚本）
> 运行：`AGENT_STORE_BIN=target/debug/agent-store.exe bun scripts/sdk-live-oauth.ts`
> 结果：**26/26 PASS**
> 范围：WP-3 P0-C/D 的 OAuth 运行时部分。协议面全部经 SDK 公共面
> （`launchClient` + `client.connectors.*`）；宿主管理面（MCP enable）标注 `[host admin]`。

## 1. 环境与 mock 平台

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

## 2. 用例证据

### TC-OAUTH-001 标准 PKCE Loopback（8/8）

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

### TC-OAUTH-002 凭据隔离（2/2）

| 检查 | 结果 |
|---|---|
| `OA-002.no-token-in-public-surfaces` | 5 个公共面（`auth/status`、`connector/get`、`connector/status`、`connector/list`、`connector/test`）扫描 `oa-access-good` / `oa-refresh-good` / 本次签发的全部 token → 命中 0 |
| `OA-002.issued-tokens-scanned` | 本次共签发 3 个凭据串，全部纳入扫描 |

### TC-OAUTH-004 错误边界（10/10）

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

## 3. 本次 live 逼出的缺陷与修复

### B7：并发 `login` 覆盖共享 pending 槽（已修）

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

### B8：动态注册身份未落盘（已修）

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

## 4. 观察与边界

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
  （探针层）覆盖，证据见 `mcp-oauth-runtime-evidence.zh.md`（2026-09-01/02，含
  QQ Mail 真实服务闭环），未纳入本 live 脚本；live 脚本未做真实模型调用
  （探针即证明注入生效）。

## 5. 复跑

```bash
cargo build -p agent-store
cd web && AGENT_STORE_BIN=../target/debug/agent-store.exe bun scripts/sdk-live-oauth.ts
# OAUTH_KEEP_DATA=1 保留 data dir 供取证；默认关闭后自删。
```

回归：`cargo test -p nomifun-mcp`（251 + 集成全绿）、
`cargo test -p nomifun-ai-agent --test mcp_oauth_e2e`。
