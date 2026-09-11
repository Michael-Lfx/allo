# TypeScript SDK 使用指南

Flowy Agent Store 提供三个配套的 TypeScript 包，让 Node.js / Electron / 浏览器应用以类型安全的方式接入本地 App Server：

| 包 | 职责 | 运行环境 | 依赖 |
| --- | --- | --- | --- |
| `@flowy-agent-store/protocol` | 协议线类型（请求/响应/通知/错误） | 任意（零运行时、无 DOM/Node） | 无 |
| `@flowy-agent-store/client` | `AppServerClient` + 7 个子客户端 + `Transport` 抽象 | 任意（无 HTTP、无 DOM、无 Node） | `@flowy-agent-store/protocol` |
| `@flowy-agent-store/sdk` | spawn `flowy-agent-store` 二进制 → 回环 WS 建连 → 就绪客户端 | Node.js（依赖 `node:child_process` 等） | `@flowy-agent-store/client`、`@flowy-agent-store/protocol` |

三个包按需组合：**只用类型**取 `protocol`；**连已运行的 App Server**（如桌面端已启动）取 `client` + 自建 `WebSocketTransport`；**自己拉起整个运行时**取 `sdk` 的 `launchClient`。

> **本文面向开发者。** 终端用户不需要它——下载安装包并按[快速开始](/zh-CN/docs/quick-start)运行即可。两条路径分工明确：**终端用户 → 安装包 / `install.ps1`**；**开发者 → npm 包（本文）**。

---

## 1. 安装

```bash
# 通常只需要 sdk（它 re-export client 的能力并自带 spawn）
bun add @flowy-agent-store/sdk        # 或 npm install / pnpm add

# 需要协议类型时显式声明
bun add @flowy-agent-store/protocol
```

包均发布为 ESM + CJS 双格式（`exports` 提供 `import` / `require` / `types`），Node 与打包器开箱即用。

> **版本状态**：三个包当前均为 `0.1.0-beta.*` 预发布（API 尚未冻结，beta 期间**不承诺向后兼容**）。生产接入请固定**确切版本**——本文与仓库当前对应 `0.1.0-beta.3`。注意不要依赖裸 `bun add`：注册表 `latest` 当前指向 `0.1.0-beta.2`，**不是**最新的 `0.1.0-beta.3`。dist-tag 语义、逐版本升级步骤与自查命令见[升级与迁移指引](/zh-CN/docs/upgrade)。
> **运行环境**：Node.js **≥ 22**（依赖全局 `WebSocket`）或 Bun；版本下限由各包 `engines.node` 声明。

---

## 2. 快速开始（SDK 一行拉起）

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const session = await launchClient({
  client: { name: "my-app", version: "0.1.0" },
});
const store = await session.client.listStore();
await session.close();
```

`launchClient` 完成的事：

1. 按 `bin` → `AGENT_STORE_BIN` → `PATH` 定位 `flowy-agent-store` 可执行文件；
2. 以 `--host 127.0.0.1 --port 0 --no-open` 并携带自动创建的临时 `--data-dir` 启动子进程；
3. 扫描 stdout 就绪行（`{"agent_store":"listening",...}`），取得实际端口；
4. **校验就绪行 `protocol_version` 与 SDK 一致**，不一致则杀进程并报错（含两端版本）；
5. 建立回环 WebSocket、执行 `initialize` → `initialized` 握手，返回可用的 `AppServerClient`。

### 完整生命周期示例

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const session = await launchClient({ client: { name: "demo", version: "1.0.0" } });
try {
  // 目录（Store）
  const items = await session.client.listStore();
  console.log(`${items.items.length} items in the store`);

  // 安装并运行一个 Agent
  await session.client.installStoreEntry("experts", "frontend-backend-experts");
  const receipt = await session.client.runs.agent({
    agentId: "frontend-backend-experts",
    goal: "Generate a todo REST API",
  });
  const result = await session.client.runs.result(receipt.run_id);
  console.log(result.status);
} finally {
  await session.close(); // 终止子进程 + 删除临时 data-dir
}
```

---

## 3. `@flowy-agent-store/protocol` — 协议层

### 3.1 定位

线协议的唯一 TypeScript 真源：所有请求/响应/通知类型、`APP_SERVER_PROTOCOL_VERSION` 常量与结构化错误。**无任何运行时代码**，可被 client/sdk/Rust 之外任何方言消费。

### 3.2 主要导出

| 导出 | 说明 |
| --- | --- |
| `APP_SERVER_PROTOCOL_VERSION` | 当前协议版本字符串（如 `"2026-08-26"`），握手与 SDK 校验用 |
| `InitializeRequest` / `InitializeResult` | 握手请求/响应（含 `protocol_version`、`server` 信息） |
| `ClientInfo` / `ClientCapabilities` | 连接方自述 |
| `StoreList` / `StoreInstallResult` | winget 式统一目录 |
| `AgentSummary` / `AgentDetail` | AgentDefinition 目录视图 |
| `TeamSummary` / `TeamDetail` | AgentTeamDefinition 目录视图 |
| `SkillSummary` / `SkillDetail` | Skill 目录视图 |
| `ConnectorSummary` / `ConnectorDetail` / `ConnectorStatusView` / `ConnectorProbeResult` | Connector 目录/状态/探测 |
| `OAuthStartResult` / `OAuthStatusView` | OAuth 浏览流状态 |
| `ConversationView` / `ConversationMessage` / `ConversationEvent` / `ConversationSendReceipt` | 持久会话 |
| `RunReceipt` / `RunView` / `RunResult` / `RunEvent` | Run 生命周期 |
| `JsonRpcRequest` / `JsonRpcResponse` / `JsonRpcNotification` | 线框类型 |
| `ServerNotification` | 服务器下行通知（`event`、`conversation/event`、`run/resync-required` 等） |
| `WireError` | 服务器错误载荷 |

> 实验性能力（Team 完整协作、事件 cursor 追平）在协议中标注 `experimental`，不进稳定导出。

### 3.3 错误模型（包内 `errors.ts`）

调用方**必须按稳定 `code` 分支，绝不解析人类可读 message**：

| 类型 | 触发 | 关键字段 |
| --- | --- | --- |
| `AppServerError` | 服务器返回 JSON-RPC error | `code`、`request_id`、`retryable`、`details` |
| `TransportError` | 传输层（连接/发送/关闭） | `phase`（connect/send/receive/close）、`retryable` |
| `ProtocolError` | 本地协议校验失败 | `kind`（`invalid_message` / `version_mismatch` / `unexpected_response`） |
| `RequestTimeoutError` | 请求超时 | `method`、`timeoutMs` |

辅助判定：

```ts
import { isAppServerError, isRetryableTransportError, formatError } from "@flowy-agent-store/protocol";

try {
  await client.runs.agent({ agentId, goal });
} catch (error) {
  if (isAppServerError(error)) {
    // 稳定 code（如 version_mismatch / marketplace_not_found），勿用 message
    console.log(error.code, error.retryable);
  } else if (isRetryableTransportError(error)) {
    // 连接断开、可重试
  }
  console.log(formatError(error)); // UI 唯一共享的错误渲染
}
```

> 幂等冲突与策略拒绝**永不自动重试**（`retryable: false`）——反复重放会叠多次副作用。

---

## 4. `@flowy-agent-store/client` — 传输无关客户端

### 4.1 定位

纯业务层：任何方法都只经注入的 `Transport`，包内无 HTTP、无 DOM、无 Node。连接生命周期（`connect → initialize → 版本检查 → initialized → ready`）全在此层完成，业务代码永远不知道底层是 WebSocket、stdio 还是未来的一次性 HTTP 绑定。

### 4.2 `Transport` 接口

```ts
export interface Transport {
  connect(): Promise<void>;                          // 建立通道（幂等）
  request<T>(method: string, params: unknown): Promise<T>;  // 请求-响应
  notify(method: string, params: unknown): void;     // 通知（无响应）
  onNotification(listener: NotificationListener): () => void; // 订阅下行通知，返回退订函数
  close(): void;
  onLifecycle?(listener: (state: "open" | "closed") => void): () => void; // 可选：通道生命周期
}
```

内置实现 `WebSocketTransport`（浏览器与 Node 22+/Bun 通用，使用全局 `WebSocket`）：

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport(
  "ws://127.0.0.1:8787/api/app-server/ws",
  { requestTimeoutMs: 30_000, connectTimeoutMs: 10_000, token: "optional-bearer" } // token 浏览器里走 ?token= 查询参数
);
const client = new AppServerClient({ transport, client: { name: "my-app", version: "0.1.0" } });
await client.connect(); // initialize 握手 + 版本校验
```

`WebSocketTransport` 的实现契约（A2 / T8）：

- **`connect()` 幂等且并发安全**：并发调用共享同一次建连（不会开出第二个 socket）；超过 `connectTimeoutMs`（默认 10s）仍未打开，则 reject `TransportError(phase: "connect", retryable: true)` 并关闭该 socket。
- **`close()` 是终态清理**：结算所有挂起请求与在途 `connect()`（`TransportError(phase: "close")`），并清空经 `onNotification` 注册的监听器。因此**关闭后重新 `connect()` 必须重新注册监听器**（`AppServerClient` 会自动重挂其通知桥，自建传输的调用方需自行处理）。
- **陈旧连接隔离**：被替换或已关闭的 socket，其迟到事件一律忽略，不会影响当前连接。
- **不自动重连**：断线只让挂起请求以 `TransportError(retryable: true)` 失败；重连策略由调用方决定。
- **重连可观测（T8）**：`onLifecycle` 在每次成功建连后报 `open`；仅在**已建立的连接丢失**时报 `closed`（首次拨号失败不算断线，调用方主动 `close()` 也不报）。`closed → open` 即一次重连。这类监听器**不随 `close()` 清空**——它们正是用来驱动重连的。
- **重连后必须重新握手并重订阅**：新 socket 意味着服务端订阅与本地 `onNotification` 监听器都已失效，所以重连流程是 `transport onLifecycle("open")` → `client.connect()`（重跑 `initialize` 握手）→ 对每个存活的订阅调用 `rearm()`。

自建传输只需实现接口即可：测试用内存假传输、CLI 用 stdio、Electron 主进程用 Node WebSocket——业务代码零改动。

### 4.3 `AppServerClient` 顶层方法

| 方法 | 线方法 | 说明 |
| --- | --- | --- |
| `connect()` | `initialize` + `initialized` | 握手；成功后 `ready === true`。协议版本不匹配抛 `ProtocolError(version_mismatch)` |
| `close()` | — | 关闭传输，服务端立即吊销连接 |
| `onNotification(listener)` | — | 全局下行通知订阅（返回退订函数） |
| `ready` / `initializeInfo` | — | 是否就绪 / 握手结果 |
| `runImport(input)` · `listImports()` · `getImport(snapshotId)` | `import/*` | 本地 CodeBuddy/WorkBuddy 目录导入 |
| `runInstall(input)` · `getInstallStatus(snapshotId)` | `install/run` / `install/status` | 快照安装 |
| `disableInstall(snapshotId, ids)` · `enableInstall(...)` · `uninstallInstall(...)` | `install/*` | 组件启停/卸载 |
| `addMarketplace(input)` · `listMarketplaces()` · `getMarketplace(id)` | `market/*` | 市场源管理 |
| `removeMarketplace(id, cascade)` | `market/remove` | `cascade=true` 时级联卸载该市场安装的快照 |
| `setMarketplaceAutoUpdate(id, enabled)` | `market/auto-update` | 自动更新开关（DB 标记） |
| `refreshMarketplace(id)` | `market/refresh` | 拉取源并重建条目（版本变化时） |
| `importMarketplaceEntry(mkt, entry)` | `market/entry-import` | 单条目导入（带 provenance） |
| `listStore()` | `store/list` | 全市场统一目录（含安装状态） |
| `installStoreEntry(mkt, entry)` | `store/install-entry` | 一键安装：缺导入就导入 + 注册 |

> ℹ️ **首次 `listStore()` 可能返回空或不完整的目录——这是设计如此，不是错误**：内置默认市场的注册在**后台**进行（D-SDK-1 ①）。首个 `store/list` 只负责触发它，然后立即用**当前已注册**的内容作答，不会等待镜像完成。
> 注册本身要把市场源整棵树从镜像站 HTTP 下载到本地（数百个技能目录 + 上千个资产），全新 data-dir 上约 **90 秒**，这段时间内该调用返回 `items: 0`。
> 实测（2026-09-10，本地全新 data-dir）：`first store/list: 1ms items=0` → 130 秒后 `store/list: 133ms items=438`、`market/list count=3`。
> 因此：**不需要为首次调用加大 `requestTimeoutMs`**；要完整目录请在预热后**重新调用**（WebUI 有显式刷新）。若镜像源不可达，本次热身记为不完整，下一次 store/market 调用会自动重试。
> 想区分「真的没有市场」与「仍在载入」：`listStore()` 的返回（`store/list`）现在带 `markets_pending` —— 为 `true` 时表示内置市场仍在后台注册、目录可能不完整。
> 若你自持 `dataDir`，同一目录的后续调用走幂等短路，不再联网。

### 4.4 子客户端

构造即绑定同一传输；所有方法返回 `Promise<T>`。

#### `agents` — AgentDefinition 目录

```ts
client.agents.list(): Promise<AgentSummary[]>;
client.agents.get(agentId: string): Promise<AgentDetail>;
```

#### `teams` — AgentTeamDefinition 目录

```ts
client.teams.list(): Promise<TeamSummary[]>;
client.teams.get(teamId: string): Promise<TeamDetail>;
```

#### `skills` — Skill 目录

```ts
client.skills.list(): Promise<SkillSummary[]>;
client.skills.get(skillId: string): Promise<SkillDetail>;
```

#### `connectors` — Connector 目录 / 状态 / OAuth

```ts
client.connectors.list(): Promise<ConnectorSummary[]>;
client.connectors.get(connectorId: string): Promise<ConnectorDetail>;        // 命名空间工具 + 认证态
client.connectors.status(connectorId: string): Promise<ConnectorStatusView>; // connected 仅在认证就绪且最近探测成功
client.connectors.test(connectorId: string): Promise<ConnectorProbeResult>;  // 运行连接探测（结果持久化）
client.connectors.authStatus(connectorId: string): Promise<OAuthStatusView>;
client.connectors.authStart(connectorId: string): Promise<OAuthStartResult>; // 发起宿主浏览器 OAuth 流，轮询 authStatus 至 authenticated
client.connectors.logout(connectorId: string): Promise<void>;                // 吊销令牌
```

> Token 永不经过此包：OAuth 浏览器流由可信宿主持有，客户端只触发与轮询。

#### `conversations` — 持久会话

```ts
client.conversations.create(input): Promise<ConversationView>;  // model 省略时由服务端按 config.toml 解析默认模型
client.conversations.update(id, input): Promise<ConversationView>;
client.conversations.modelOptions(): Promise<ConversationModelOptions>;
client.conversations.list(limit = 100): Promise<ConversationView[]>;
client.conversations.get(id): Promise<ConversationView>;
client.conversations.messages(query): Promise<ConversationMessagesPage>; // page/page_size/cursor
client.conversations.send(id, content, idempotencyKey): Promise<ConversationSendReceipt>; // 必须显式幂等键
client.conversations.cancel(id): Promise<ConversationView>;
client.conversations.delete(id): Promise<{ conversation_id: string; deleted: boolean }>;
await client.conversations.follow(id): Promise<ConversationSubscription>;
```

`modelOptions()` 的每个模型条目除 `name` / `display_name` / `context_limit` 外，还可能带 **models.dev 目录事实**：`cost_input` / `cost_output`（每百万 token 的 USD 费率）、`catalog_context_window`、`supports_vision`。**目录没有对应条目时这些字段整个缺席**（provider 未被映射，或模型不在目录里）——调用方应把它们当作「不知道」，而不是 `false` 或 `0`。

实时订阅对象：

```ts
const sub = await client.conversations.follow(convId);
sub.onEvent((event) => console.log("seq", event.sequence, event)); // 自动按 sequence 去重
sub.onResync((reason) => console.log("resync required:", reason)); // 断网追平提示
sub.lastSequence; // 已见最大序号
await sub.rearm(); // 重连后：重注册监听 + 游标归零 + 重发 conversation/subscribe
await sub.close(); // 服务器端退订（也可靠关闭 socket 隐式退订）
```

> `rearm()` 之后仍需自行补齐断线窗口的正文：该订阅没有事件重放接口，请用 `conversation/messages` 重新拉取（游标归零意味着后续重复事件由调用方按 `sequence` 去重）。

#### `runs` — Run 生命周期与实时事件

```ts
client.runs.agent(input: AgentRunInput): Promise<RunReceipt>; // 异步回执，非最终结果
client.runs.get(runId): Promise<RunView>;                     // 权威状态
client.runs.result(runId): Promise<RunResult>;                // 终态后才成功
client.runs.events({ runId, afterSequence, limit }): Promise<RunEvent[]>; // 游标重放
client.runs.cancel({ runId, expectedVersion, commandId, idempotencyKey }): Promise<RunView>;
await client.runs.follow(runId): Promise<EventSubscription>;
```

```ts
const sub = await client.runs.follow(runId);
sub.onEvent((event) => console.log(event));       // 尽力而为实时事件（可丢、可乱序）
sub.onResync(({ run_ids, reason }) => …);         // 订阅失效要求重放
sub.onError((error) => …);                        // 传输错误转发
sub.lastSequence;
const replayed = await sub.rearm();               // 重连后：游标归零 + 重订阅 + 全量重放
await sub.close();
```

> `rearm()` 会**重放全部历史事件**（游标归零是刻意的），因此消费方必须按 `sequence` 去重；返回值即本次重放的事件。只应在重连握手（`initialize`）完成后调用。

> 事件语义是尽力而为：持久性依赖 `run/events` 游标重放，节点实现需自行去重排序。

#### `workspaces` — 工作区注册

```ts
client.workspaces.list(): Promise<WorkspaceView[]>;
client.workspaces.create(path: string): Promise<WorkspaceView>; // 服务端 canonicalize + 拒绝链接/重解析点
client.workspaces.revoke(workspaceId: string): Promise<WorkspaceRevokeResult>; // 软删除，会话保留
```

---

## 5. `@flowy-agent-store/sdk` — Node 宿主

### 5.1 `launchClient(options): Promise<LaunchedClient>`

```ts
interface LaunchOptions extends SpawnOptions {
  client: ClientInfo;             // { name, version }
  capabilities?: ClientCapabilities;
  token?: string;                 // 传给 WebSocketTransport
  requestTimeoutMs?: number;      // 默认 30s（不足以覆盖首次 store/list，见 §4.3 警告）
}

interface LaunchedClient {
  server: SpawnedServer;          // readiness / dataDir / exited / close
  client: AppServerClient;        // 已握手就绪
  initializeResult: InitializeResult;
  close(): Promise<void>;         // 退订 → 关传输 → 终止子进程 → 删临时 data-dir
}
```

### 5.2 底层原语

| 导出 | 说明 |
| --- | --- |
| `spawnAppServer(options: SpawnOptions)` | 仅 spawn + 等就绪行（不建连）。`SpawnOptions`: 见下 |
| `resolveAppServerBin(explicit?)` | 定位二进制（见 §5.3 ） |
| `parseReadinessLine(line)` | 解析单行；非就绪行返回 `null` |
| `ReadinessInfo` | `{ host, port, url, protocol_version, version, auth }` |
| `assertProtocolCompatible(runtimeVersion)` | 版本不一致直接 throw（含两端版本） |
| `SpawnExitInfo` | `{ code, signal }`——子进程如何退出（`exited` / `onExit` 的载荷） |

`SpawnOptions`：

```ts
interface SpawnOptions {
  bin?: string;            // 显式路径（覆盖一切）
  dataDir?: string;        // 自持 data-dir；省略则自动临时目录（close 时删除）
  port?: number;           // 默认 0 = 系统分配
  extraArgs?: string[];    // 追加 CLI 参数
  readyTimeoutMs?: number; // 默认 120s（冷启动建库）
  env?: Record<string, string | undefined>; // 在 process.env 之上合并
  cwd?: string;            // 子进程工作目录；省略即继承父进程
  onExit?: (info: SpawnExitInfo) => void;   // 子进程退出时回调一次
}
```

`SpawnedServer.exited` 是一个**永不 reject** 的 `Promise<SpawnExitInfo>`，在子进程因任意原因退出时 settle——这是观察运行时崩溃的唯一入口。

### 5.3 二进制定位

顺序：`bin` 参数 → 环境变量 `AGENT_STORE_BIN` → `PATH` 上的 `flowy-agent-store` / `flowy-agent-store.exe`。找不到**直接报错、绝不下载或猜测**（release 资产下载属 P2）。

```bash
AGENT_STORE_BIN=/opt/flowy-agent-store/flowy-agent-store node your-app.mjs
```

### 5.4 运行契约（P0 实测结论）

- **回环强制**：子进程固定 `--host 127.0.0.1 --no-open`；SDK 也只连刚 spawn 的进程（`isLoopbackUrl` 非回环一律拒绝）。
- **data-dir 独占**：省略 `dataDir` → 自动 `mkdtemp` 临时目录，`close()` 时删除；传入自己的目录即表示独占——后端单实例锁会 fail-fast（`already in use by another running Flowy backend`）。
- **版本校验**：就绪行 `protocol_version` 与 SDK 不符立即杀进程报错（含两端版本号）。
- **就绪行格式**：子进程 stdout 单行 JSON `{"agent_store":"listening","host":...,"port":...,"url":...,"protocol_version":...,"version":...,"auth":...}`；SDK 逐行扫描、忽略其他行（tracing 也走 stdout）。
- **stdout 持续排空**：就绪行解析完成后，SDK 继续读取并丢弃子进程 stdout（`readline.close()` 会 `pause` 该流，所以不能就此停止读取）。否则运行时日志写满 OS 管道缓冲（约 64KB）后会永久阻塞在写上，长会话（多轮 turn、市场树扫描）表现为静默卡死。后续输出仅被排空丢弃，本轮不提供日志回调。
- **`env` / `cwd` 透传**：`env` 在父进程 `process.env` 之上**合并**（不是替换，`PATH` 等仍可见）；`cwd` 省略即继承父进程工作目录。两者原样交给 `child_process.spawn`。
- **退出可见**：`SpawnedServer.exited`（`{ code, signal }`）在子进程**任意原因退出**时 settle，含崩溃与非零退出码；`onExit` 同时触发一次。SDK **不自动重启**，重启用 `launchClient` 的调用方负责。

### 5.5 错误与清理

- spawn 失败：报错附 **stderr 尾部 50 行**（`stderr tail:` 段）。
- 超时：默认 120s 后抛 `timed out waiting for the runtime readiness line`。
- 任何失败路径都会 `child.kill()` → 2s 宽限 → `SIGKILL`，并删除自动创建的 data-dir。
- 就绪成功后 promise 已结算：此后子进程再 `exit` / `error` 不再走失败路径（不会被当成启动失败）；这类退出（含崩溃）只通过 `SpawnedServer.exited` 与 `onExit` 暴露，SDK 不自动重启，生命周期由调用方以 `close()` 负责。
- 正确用法：`try/finally` 中 `close()`；进程退出时若未 close，临时目录会残留（SDK 不装退出钩子）。

```ts
const session = await launchClient({ client: { name: "x", version: "1" } });
try {
  await session.client.connectors.list();
} finally {
  await session.close();
}
```

---

## 6. 完整场景：浏览器接入桌面端

桌面端已启动 App Server 时，浏览器直接建连（无需 sdk）：

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const ws = new WebSocketTransport(`ws://127.0.0.1:8787/api/app-server/ws`);
const client = new AppServerClient({ transport: ws, client: { name: "web", version: "1.0.0" } });
await client.connect();
console.log("connected:", client.ready);
```

> 注意自定义 header 在浏览器 WebSocket 中不可用：`WebSocketTransport` 会把 `token` 追加为 `?token=` 查询参数；Node 侧 HTTP 走 `Authorization` 头。

---

## 7. 逐方法 API 参考

三个包的实际导出面与协议方法的对齐关系。协议方法名以 `05` 为准，此表不引入新方法。

### 7.1 `AppServerClient` 顶层方法

| 方法 | 参数 | 返回 | 协议方法 |
| --- | --- | --- | --- |
| `connect()` | — | `InitializeResult` | `initialize` → `initialized` |
| `onNotification(listener)` | `(notification) => void` | 取消订阅函数 | —（服务端通知） |
| `close()` | — | `void` | — |
| `runImport(input)` | `ImportRequest` | `ImportResult` | `import/run` |
| `listImports()` | — | `ImportSummary[]` | `import/list` |
| `getImport(snapshotId)` | `string` | `ImportDetail` | `import/get` |
| `runInstall(input)` | `InstallRequest` | `InstallResult` | `install/run` |
| `getInstallStatus(snapshotId)` | `string` | `InstallStatus` | `install/status` |
| `disableInstall(snapshotId, componentIds)` | `string, string[]` | `InstallStatus` | `install/disable` |
| `enableInstall(snapshotId, componentIds)` | `string, string[]` | `InstallStatus` | `install/enable` |
| `uninstallInstall(snapshotId, componentIds)` | `string, string[]` | `InstallStatus` | `install/uninstall` |
| `addMarketplace(input)` | `MarketplaceAddRequest` | `MarketplaceSummary` | `market/add` |
| `listMarketplaces()` | — | `MarketplaceSummary[]` | `market/list` |
| `getMarketplace(marketplaceId)` | `string` | `MarketplaceDetail` | `market/get` |
| `removeMarketplace(marketplaceId, cascade)` | `string, boolean` | `MarketplaceRemoveResult` | `market/remove` |
| `setMarketplaceAutoUpdate(marketplaceId, enabled)` | `string, boolean` | `MarketplaceSummary` | `market/auto-update` |
| `refreshMarketplace(marketplaceId)` | `string` | `MarketplaceRefreshResult` | `market/refresh` |
| `importMarketplaceEntry(marketplaceId, entryName)` | `string, string` | `ImportResult` | `market/entry-import` |
| `listStore()` | — | `StoreList` | `store/list` |
| `installStoreEntry(marketplaceId, entryName)` | `string, string` | `StoreInstallResult` | `store/install-entry` |

### 7.2 子客户端

| 子客户端 | 方法 | 协议方法 |
| --- | --- | --- |
| `agents` | `list()` / `get(agentId)` | `agent/list` / `agent/get` |
| `teams` | `list()` / `get(teamId)` | `team/list` / `team/get` |
| `skills` | `list()` / `get(skillId)` | `skill/list` / `skill/get` |
| `connectors` | `list()` / `get(id)` / `status(id)` / `test(id)` / `authStatus(id)` / `authStart(id)` / `logout(id)` | `connector/list` · `get` · `status` · `test` · `auth/status` · `auth/start` · `auth/logout` |
| `conversations` | `create(input)` / `update(id, input)` / `modelOptions()` / `list(limit?)` / `get(id)` / `messages(query)` / `send(id, content, idempotencyKey)` / `cancel(id)` / `delete(id)` / `follow(id, options?)` | `conversation/*` 同名方法 |
| `runs` | `agent(input)` / `get(id)` / `result(id)` / `events(query)` / `cancel(input)` / `steer(input)` / `answerDecision(input)` / `follow(id, options?)` | `agent/run` · `run/get` · `run/result` · `run/events` · `run/cancel` · `run/steer` · `run/answer-decision` |
| `workspaces` | `list()` / `create(path)` / `revoke(id)` | `workspace/list` / `workspace/create` / `workspace/revoke` |
| `models` | `list()` | `models/list` |

### 7.3 HTTP 绑定

HTTP 与 WebSocket 是同一套方法语义的两种绑定。包内 `httpRouteTable()` 返回**机器可读的路由表**（方法名 → 动词 + 路径 + 证据来源），文档不再手抄一份：

```ts
import { httpRouteTable } from "@flowy-agent-store/client";

const routes = httpRouteTable();
// { "market/remove": { verb: "POST", path: "/markets/:marketplace_id/remove", source: "…" }, … }
```

- 覆盖 **45 / 64** 个方法。HTTP 无绑定的 19 个方法：`initialize`、`initialized`、`workspace/create`、`conversation/model-options`、`conversation/update`、`conversation/subscribe`、`conversation/unsubscribe`、`run/subscribe`、`run/unsubscribe`、`agent/list`、`agent/get`、`team/list`、`team/get`、`config/get`、`config/set`、`skill/create`、`skill/update`、`skill/delete`、`skill/copy`。
- `config/get` / `config/set`（宿主设置文件 `~/.agent-store/config.toml`）是**宿主管理面**（`16` §6）：只有 wire 方法，没有 HTTP 绑定，也**不在本包客户端内**——Web UI 自己经 transport 调用。契约见 `05` §4.10。
- `skill/create` / `skill/update` / `skill/delete` / `skill/copy`（技能写面，`16` R17 / W12）同样按 `16` §6 判定为**宿主管理面**：第三方消费者不应能往宿主的技能树里写文件，因此只有 wire 方法、没有 HTTP 绑定，也不在本包客户端内。`skill/update` 是**字段级补丁**（只改点名的字段，`name` 不可改），`skill/copy` 从任意来源派生一份可写的用户技能。读面的 `SkillSummary` 新增 `origin` / `writable` 两个字段（增量），契约见 `05` §4.11。
- **`HttpTransport` 是请求-响应面，不等价于 WebSocket**：`notify()` 抛错、`onNotification()` 返回空订阅。实时事件与订阅必须走 `WebSocketTransport`。
- 每次调用独立握手（`initialize` → `initialized` → 业务调用）；`connect()` 是 no-op。宿主侧服务若需要就绪连接 id，用 `openConnection()`。
- `/api/fs/*`（浏览 / 列表 / 读取 / 元数据）是宿主文件服务，不是协议方法，不在本包内。

### 7.4 审批回答：`run/answer-decision`

Agent 运行到需要人决策时会停下来，`run/events` 投影出 `approval.requested`，回答走 `runs.answerDecision(input)`：

```ts
const pending = (await client.runs.events({ runId })).find(
  (event) => event.event_type === "approval.requested",
);

await client.runs.answerDecision({
  runId,
  stepId: pending.step_id!,                    // 事件投影的 attempt 作用域
  attemptId: pending.attempt_id!,
  answer: "批准，继续执行",
  expectedExecutionVersion: pending.expected_execution_version!,  // 三个 CAS 版本
  expectedStepVersion: pending.expected_step_version!,
  expectedAttemptVersion: pending.expected_attempt_version!,
});
```

- **三个 `expected*Version` 是必填的 CAS 令牌**，不是可选优化：服务端把它们直通引擎的唯一回答门，三个版本中任意一个已变化即返回 `conflict`，绝不静默覆盖。`run/events` 会在每条未回答的 `approval.requested` 上投影当前三个版本（读取时从权威行取），客户端照抄即可。
- **只有 `waiting_input` 的 attempt 能被回答**；越权、已过期、非等待态一律拒绝（`NotFound` / `Conflict` / `BadRequest`）。
- **没有 `always_allow`**：桌面端确认路由上的 approve-all 开关不属于本协议，方法参数是 `deny_unknown_fields`，带上它直接报 `invalid_request`。
- `RunEvent` 的 `step_id` / `attempt_id` 只在引擎按 attempt 归属事件时出现（典型是 `approval.requested` / `approval.responded`）。

## 8. 事件参考：`sequence` 与追平

### 8.1 事件类型

`ConversationEventType` 是**封闭联合**（`protocol.ts`），共 9 种：

| 事件类型 | 含义 | 解码后 kind |
| --- | --- | --- |
| `message.created` | 新消息落库 | `message.created` |
| `message.delta` | 正文增量（`replace` 为真时整体替换） | `message.delta` |
| `message.thinking` | 思考段落增量 | `message.thinking` |
| `message.tips` | 提示条（`tip_type`） | `message.tips` |
| `message.tool` | 工具调用（running → completed 分帧） | `message.tool` |
| `message.error` | 终止性错误（解码后带 `code` 与 `retryable`） | `message.error` |
| `message.activity` | 活动条目（`kind` 决定渲染；`kind === "turn_completed"` 时带本轮 token 用量） | `message.activity` |
| `turn.status` | 轮次忙闲（`status === "running"`） | `turn.status` |
| `context.usage` | 上下文用量 | `context.usage` |

服务端用两种拼写表示「思考」：`message.thinking` 与 `message.activity` 且 `kind === "thinking"`；`decodeConversationEvent` 把后者**归一化**为 `message.thinking`，调用方只需一条思考路径。未知类型落到 `unknown`（保留原始 `event_type`），不会被误判为已知识别类型。

`message.activity` 且 `kind === "turn_completed"` 时会带**本轮** token 用量（`usage: { input_tokens, output_tokens, total_tokens }`，来自运行时的逐轮上报）；运行时就**没上报**、只报了单侧或两侧皆为 0 时该字段为 `null`——**「未知」不等于「不花钱」**，调用方不得拿上下文占用或 0 顶替。逐轮用量只随实时事件到达（服务端不持久化历史轮次），且字段名为 snake_case，与 Run 面的 `TurnUsage` 一致。

`message.error` 除错误文本外还解码出 `code`（服务端错误码）与 `retryable`（**三态**：`true` / `false` / `null`——`null` 表示 wire 未提供，例如历史行，调用方不得把它当作 `false` 或 `true` 猜着用）。是否需要重试由调用方按这两项决定；`conversation/send` 的响应另带 `result_error_retryable`，两者一致。

### 8.2 `sequence` 语义

- `sequence` 是**单会话内单调自增且连续**的计数器（服务端按会话维护），不是全局序号。
- 取消订阅后该计数器销毁；重新订阅从 `1` 开始，因此 `rearm()` 会把本地游标重置为 `0`。
- 缺口判定：收到 `sequence > lastSeen + 1` 且 `lastSeen > 0` 即判定丢帧，订阅会发出 `onResync("gap")` 并触发追平。
- 重复与乱序（`sequence <= lastSeen`）直接丢弃，不重复投递。

### 8.3 追平（catch-up）

会话与 Run 的追平载体不同：

| 场景 | 服务端信号 | 追平手段 | 包内入口 |
| --- | --- | --- | --- |
| 会话 | `conversation/resync-required` | `conversation/messages` 重新拉取（V1 不提供会话事件回放） | `follow(..., { fetchMessages })` → `onBackfill` |
| Run | `run/resync-required` | `run/events` 带 `after_sequence` 回放 | `follow()` 自动追平；`resync()` / `catchUp()` 手动 |

会话订阅默认自动追平（`autoResync`），一次只跑一个取数请求（突发信号合并）；拉取到的新页经 `onBackfill` 交给上层。若上层自己持有分页游标，传 `autoResync: false` 并只监听 `onResync`，由上层做权威重载。

在已 `connect()` 的 `client` 上（完整装配见 §10）：

```ts
const subscription = await client.conversations.follow(conversationId);
subscription.onEvent((event) => {
  const decoded = decodeConversationEvent(event);
  if (decoded.kind === "message.delta") render(decoded.delta, decoded.replace);
});
subscription.onBackfill((snapshot) => resetTranscript(snapshot.messages));
subscription.onError((error) => report(error));
```

## 9. 错误模型与重试

四个错误类都从 `@flowy-agent-store/protocol` 导出，`retryable` 是稳定契约（不要按 `message` 分支）：

| 类 | 出现场景 | `retryable` |
| --- | --- | --- |
| `AppServerError` | 服务端返回的业务错误；带 `code` / `request_id` / `details` | 由服务端 hint 决定 |
| `TransportError` | 连接、发送、接收、关闭失败；带 `phase` | 由 `phase` 与调用方判定 |
| `ProtocolError` | 报文不合规、版本不匹配、响应不符合预期；带 `kind` | 否 |
| `RequestTimeoutError` | 请求超时；带 `method` / `timeoutMs` | 否 |

`isRetryableError(error)` 统一判定是否需要重试；`formatError(error)` 给出唯一的人读文案。`withRetry(operation, options)` 按 `retryable` 指数退避（含抖动）：

| 选项 | 默认 | 说明 |
| --- | --- | --- |
| `maxAttempts` | `3` | 含首次在内的总尝试次数 |
| `baseDelayMs` | `500` | 首次退避 |
| `maxDelayMs` | `8000` | 单次退避上限 |
| `jitter` | `0.25` | 抖动比例，延迟落在 `[0.75×, 1.0×]` |
| `onRetry` | — | 每次重试前回调 `{ attempt, delayMs, error }` |
| `shouldRetry` | 协议 `retryable` | 自定义判定 |
| `sleep` | `setTimeout` | 注入用（测试） |

```ts
import { withRetry } from "@flowy-agent-store/client";

const view = await withRetry(() => client.runs.get(runId), {
  maxAttempts: 4,
  onRetry: ({ attempt, delayMs }) => log(`retry ${attempt} in ${delayMs}ms`),
});
```

带 `idempotency_key` / `command_id` 的写操作可安全重放：App Server 对同键请求去重，不会重复执行。

## 10. 示例集：Node / 浏览器 / Electron

### 10.1 Node：一行拉起本地运行时

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const launched = await launchClient({ client: { name: "my-tool", version: "1.0.0" } });
try {
  const conversation = await launched.client.conversations.create({ name: "demo" });
  const subscription = await launched.client.conversations.follow(conversation.conversation_id);
  subscription.onEvent((event) => console.log(event.event_type));
  await launched.client.conversations.send(conversation.conversation_id, "你好", crypto.randomUUID());
} finally {
  await launched.close();
}
```

### 10.2 浏览器：只连已运行的服务端

浏览器不 spawn 进程，只连 WebSocket；`WebSocketTransport` 通过 `?token=` 传凭据（浏览器 WebSocket 不能设自定义头）。

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport("ws://127.0.0.1:8787/api/app-server/ws", { token });
const client = new AppServerClient({ transport, client: { name: "web", version: "1.0.0" } });
await client.connect();
```

### 10.3 Electron：主进程 spawn，渲染进程连回环

主进程持二进制与数据目录；渲染进程只拿回环 URL 与 token。凭据放主进程（系统凭据存储），不要进渲染进程或配置明文。

```ts
import { spawnAppServer } from "@flowy-agent-store/sdk";

const server = await spawnAppServer({ dataDir: app.getPath("userData") });
win.webContents.send("app-server-ready", {
  url: `ws://${server.readiness.host}:${server.readiness.port}/api/app-server/ws`,
});
app.on("before-quit", () => void server.close());
```

纯请求-响应场景（无实时事件）可用 `HttpTransport`；它不订阅通知，且每次调用独立握手。

## 11. MCP 接入指南

Agent Store 的官方 MCP 接入路径是**连接器描述文件**，不引入第二套格式：

| 场景 | 声明位置 |
| --- | --- |
| 连接器市场条目 | `.codebuddy-connector/connectors.json` 的条目 |
| 插件自带的 MCP server | 插件清单的 `mcpServers` 字段 |

远程 HTTP/SSE 与本地 stdio 两种 server 都按清单原样描述；Agent Store 只做托管与工具命名空间代理，不执行连接器内容。凭据处理：

- 敏感的配置项走 `userConfig` 的 schema 标记，值进操作系统凭据存储；
- 不要把密钥写进 `env`：当前版本会把 `env` 明文写入快照（已登记为待修偏差），在修复前请用 `userConfig`。

更多清单字段与示例见 [插件与市场](/zh-CN/docs/plugins-market)。

## 12. 下一步


- 协议方法语义全集：见仓库 `docs/agent-store/05-allo-app-server-protocol.md`。
- 包实现与测试样例：`web/packages/{protocol,client,sdk}/src`（SDK 含 `spawn.test.ts`、`readiness.test.ts` 用例）。
- 浏览器专属辅助（资产 `<img>` URL、`/api/fs/browse`）：宿主 app 实现，不在三包内。

---

## 13. 示例（端到端实战）

本章把前几节分散的片段整合为可直接复制运行的完整场景。所有片段都假设已通过 `launchClient`（或自建 `AppServerClient`）拿到就绪的 `client`，且用 `try/finally` 保证 `close()`。

> 实时事件类示例（会话 / Run 的 `follow`）依赖 `WebSocketTransport`；纯请求-响应场景可用 `HttpTransport`（见 §7.3）。

### 13.1 三端拉起 / 连接

**Node：一行拉起本地运行时**（SDK 负责 spawn + 回环建连 + 握手）：

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const launched = await launchClient({ client: { name: "my-tool", version: "1.0.0" } });
try {
  const conversation = await launched.client.conversations.create({ name: "demo" });
  const subscription = await launched.client.conversations.follow(conversation.conversation_id);
  subscription.onEvent((event) => console.log(event.event_type));
  await launched.client.conversations.send(conversation.conversation_id, "你好", crypto.randomUUID());
} finally {
  await launched.close(); // 终止子进程 + 删除临时 data-dir
}
```

**浏览器：只连已运行的服务端**（不 spawn 进程；凭据走 `?token=`）：

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport("ws://127.0.0.1:8787/api/app-server/ws", { token });
const client = new AppServerClient({ transport, client: { name: "web", version: "1.0.0" } });
await client.connect();
```

**Electron：主进程 spawn，渲染进程连回环**（凭据留主进程，不进渲染进程）：

```ts
import { spawnAppServer } from "@flowy-agent-store/sdk";

const server = await spawnAppServer({ dataDir: app.getPath("userData") });
win.webContents.send("app-server-ready", {
  url: `ws://${server.readiness.host}:${server.readiness.port}/api/app-server/ws`,
});
app.on("before-quit", () => void server.close());
```

> **Electron 完整接入（主进程 / 预加载 / 渲染进程）**
>
> 上面是最小骨架。真实 Electron 应用要把「运行时拉起与凭据」留在主进程，渲染进程只拿到回环 URL（和可选的 token）。凭据（OAuth 令牌、系统凭据存储）永远不要进渲染进程或配置文件明文。

**主进程 `main.ts`** — spawn 运行时、经 IPC 把连接信息交给渲染进程、退出时清理：

```ts
import { app, BrowserWindow, ipcMain } from "electron";
import { spawnAppServer, type SpawnedServer } from "@flowy-agent-store/sdk";
import { join } from "node:path";

let server: SpawnedServer | null = null;

async function startBackend() {
  server = await spawnAppServer({
    dataDir: join(app.getPath("userData"), "agent-store"),
  });

  // 回环 WS 地址（loopback 下 auth 通常为 disabled-local，无需 token）
  const wsUrl = `ws://${server.readiness.host}:${server.readiness.port}/api/app-server/ws`;
  const token = server.readiness.auth === "disabled-local" ? undefined : await getHostToken();

  // 渲染进程主动来取
  ipcMain.handle("agent-store:get-connection", () => ({ url: wsUrl, token }));

  // 崩溃可见：进程意外退出时记日志（SDK 不自动重启）
  server.exited.then((info) => {
    console.warn("agent-store runtime exited:", info.code, info.signal);
  });
}

app.whenReady().then(startBackend);

app.on("before-quit", async (event) => {
  if (server) {
    event.preventDefault(); // 先等清理完成再退出
    await server.close();
    server = null;
  }
  app.exit();
});
```

> `getHostToken()` 由宿主自己实现——只有服务端要求 token（非 `disabled-local`）时才需要；token 由主进程生成 / 获取，绝不写入渲染进程可访问的明文。

**预加载 `preload.ts`** — 用 `contextBridge` 安全地暴露给渲染进程（不暴露整个 `ipcRenderer`）：

```ts
import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("agentStore", {
  getConnection: () => ipcRenderer.invoke("agent-store:get-connection"),
});
```

**渲染进程 `renderer.ts`** — 拿到 URL 后自建 `AppServerClient` 并握手：

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const { url, token } = await window.agentStore.getConnection();
const transport = new WebSocketTransport(url, { token, requestTimeoutMs: 30_000 });
const client = new AppServerClient({ transport, client: { name: "electron-ui", version: "1.0.0" } });
await client.connect();

// 之后即可使用全部子客户端
const catalog = await client.connectors.list();
```

> 渲染进程走 `WebSocketTransport` 的 `token` 选项会被自动追加为 `?token=` 查询参数（浏览器 / Electron 的 WebSocket 不能设自定义头）。纯请求-响应场景可用 `HttpTransport`。

### 13.2 Connector OAuth 全流程

典型链路：`list` 取 `connectorId` → `authStart` 发起浏览器流 → 轮询 `authStatus` 至 `authenticated` → 用完 `logout` 吊销。

```ts
// 1) 从目录取得 connectorId
const catalog = await client.connectors.list();
const github = catalog.find((c) => c.id === "github");
if (!github) throw new Error("github connector not found in catalog");

// 2) 先看认证态：已认证则可跳过授权
const before = await client.connectors.authStatus(github.id);
if (before.state !== "authenticated") {
  // 3) 发起宿主浏览器 OAuth 流（立即返回 started，不阻塞）
  const started = await client.connectors.authStart(github.id);
  if (started.state !== "started") {
    throw new Error(started.error ?? "auth start failed");
  }

  // 4) 轮询直到 authenticated（或超时 / 需要重新授权）
  const deadline = Date.now() + 5 * 60_000; // 5 分钟宽限
  let authenticated = false;
  while (Date.now() < deadline) {
    const status = await client.connectors.authStatus(github.id);
    if (status.state === "authenticated") { authenticated = true; break; }
    if (status.state === "reauthorization_required") {
      throw new Error("reauthorization required");
    }
    await new Promise((r) => setTimeout(r, 1_500)); // 1.5s 间隔
  }
  if (!authenticated) throw new Error("oauth timed out");
}

// 5) 授权后连接器状态应为 connected（认证就绪 + 最近探测成功）
const status = await client.connectors.status(github.id);
console.log(status.status);

// 6) 用完吊销令牌
await client.connectors.logout(github.id);
```

> 注意：`authStart` 只返回 `started`，**不返回授权 URL 或 token**——浏览器流程由可信宿主持有，客户端只触发与轮询（见 §4.4）。stdio 类型连接器不支持 OAuth，服务端会报 `OAuth is not supported for stdio connectors`。

### 13.3 Store 浏览与安装

```ts
// 列出全市场统一目录（首次可能为空，含 markets_pending 标志，见 §4.3）
const store = await client.listStore();
for (const item of store.items) {
  console.log(item.marketplace_id, item.entry_name, item.kind, item.installed);
}

// 一键安装：缺导入就导入 + 注册
const receipt = await client.installStoreEntry("experts", "frontend-backend-experts");
console.log("installed:", receipt.installed);

// 市场源管理
const markets = await client.listMarketplaces();
const added = await client.addMarketplace({ source: "https://example.com/market.json" });
await client.refreshMarketplace(added.marketplace_id);
await client.removeMarketplace(added.marketplace_id, /* cascade */ true);
```

### 13.4 会话与 Run 实战

**会话：创建 → 发送 → 实时接收**

```ts
import { decodeConversationEvent } from "@flowy-agent-store/protocol";

const conv = await client.conversations.create({ name: "demo" });
const sub = await client.conversations.follow(conv.conversation_id);
sub.onEvent((event) => {
  const decoded = decodeConversationEvent(event);
  if (decoded.kind === "message.delta") render(decoded.delta, decoded.replace);
});
sub.onBackfill((snapshot) => resetTranscript(snapshot.messages));
sub.onError((error) => report(error));

const receipt = await client.conversations.send(
  conv.conversation_id,
  "帮我写个 REST API",
  crypto.randomUUID(), // 必须显式幂等键
);
```

**Run：发起 → 等待结果 → 处理审批**

```ts
const run = await client.runs.agent({
  agentId: "frontend-backend-experts",
  goal: "Generate a todo REST API",
});
const result = await client.runs.result(run.run_id); // 终态后才成功
console.log(result.status);

// 若 Agent 需要人决策，follow 实时事件并回答
const sub = await client.runs.follow(run.run_id);
sub.onEvent((event) => {
  if (event.event_type !== "approval.requested") return;
  client.runs.answerDecision({
    runId: run.run_id,
    stepId: event.step_id!,
    attemptId: event.attempt_id!,
    answer: "批准，继续执行",
    expectedExecutionVersion: event.expected_execution_version!,
    expectedStepVersion: event.expected_step_version!,
    expectedAttemptVersion: event.expected_attempt_version!,
  });
});
```

> `answerDecision` 的三个 `expected*Version` 是必填 CAS 令牌，任一变化即返回 `conflict`（见 §7.4）。带 `idempotency_key` 的写操作可安全重放。