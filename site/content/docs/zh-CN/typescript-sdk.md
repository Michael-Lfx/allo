# TypeScript SDK 使用指南

Flowy Agent Store 提供三个配套的 TypeScript 包，让 Node.js / Electron / 浏览器应用以类型安全的方式接入本地 App Server：

| 包 | 职责 | 运行环境 | 依赖 |
| --- | --- | --- | --- |
| `@flowy-agent-store/protocol` | 协议线类型（请求/响应/通知/错误） | 任意（零运行时、无 DOM/Node） | 无 |
| `@flowy-agent-store/client` | `AppServerClient` + 7 个子客户端 + `Transport` 抽象 | 任意（无 HTTP、无 DOM、无 Node） | `@flowy-agent-store/protocol` |
| `@flowy-agent-store/sdk` | spawn `agent-store` 二进制 → 回环 WS 建连 → 就绪客户端 | Node.js（依赖 `node:child_process` 等） | `@flowy-agent-store/client`、`@flowy-agent-store/protocol` |

三个包按需组合：**只用类型**取 `protocol`；**连已运行的 App Server**（如桌面端已启动）取 `client` + 自建 `WebSocketTransport`；**自己拉起整个运行时**取 `sdk` 的 `launchClient`。

---

## 1. 安装

```bash
# 通常只需要 sdk（它 re-export client 的能力并自带 spawn）
bun add @flowy-agent-store/sdk        # 或 npm install / pnpm add

# 需要协议类型时显式声明
bun add @flowy-agent-store/protocol
```

包均发布为 ESM + CJS 双格式（`exports` 提供 `import` / `require` / `types`），Node 与打包器开箱即用。

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

1. 按 `bin` → `AGENT_STORE_BIN` → `PATH` 定位 `agent-store` 可执行文件；
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
}
```

内置实现 `WebSocketTransport`（浏览器与 Node 22+/Bun 通用，使用全局 `WebSocket`）：

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport(
  "ws://127.0.0.1:8787/api/app-server/ws",
  { requestTimeoutMs: 30_000, token: "optional-bearer" } // token 浏览器里走 ?token= 查询参数
);
const client = new AppServerClient({ transport, client: { name: "my-app", version: "0.1.0" } });
await client.connect(); // initialize 握手 + 版本校验
```

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

实时订阅对象：

```ts
const sub = await client.conversations.follow(convId);
sub.onEvent((event) => console.log("seq", event.sequence, event)); // 自动按 sequence 去重
sub.onResync((reason) => console.log("resync required:", reason)); // 断网追平提示
sub.lastSequence; // 已见最大序号
await sub.close(); // 服务器端退订（也可靠关闭 socket 隐式退订）
```

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
await sub.close();
```

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
  requestTimeoutMs?: number;      // 默认 30s
}

interface LaunchedClient {
  server: SpawnedServer;          // readiness/dataDir/close
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

`SpawnOptions`：

```ts
interface SpawnOptions {
  bin?: string;            // 显式路径（覆盖一切）
  dataDir?: string;        // 自持 data-dir；省略则自动临时目录（close 时删除）
  port?: number;           // 默认 0 = 系统分配
  extraArgs?: string[];    // 追加 CLI 参数
  readyTimeoutMs?: number; // 默认 120s（冷启动建库）
}
```

### 5.3 二进制定位

顺序：`bin` 参数 → 环境变量 `AGENT_STORE_BIN` → `PATH` 上的 `agent-store` / `agent-store.exe`。找不到**直接报错、绝不下载或猜测**（release 资产下载属 P2）。

```bash
AGENT_STORE_BIN=/opt/agent-store/agent-store node your-app.mjs
```

### 5.4 运行契约（P0 实测结论）

- **回环强制**：子进程固定 `--host 127.0.0.1 --no-open`；SDK 也只连刚 spawn 的进程（`isLoopbackUrl` 非回环一律拒绝）。
- **data-dir 独占**：省略 `dataDir` → 自动 `mkdtemp` 临时目录，`close()` 时删除；传入自己的目录即表示独占——后端单实例锁会 fail-fast（`already in use by another running Flowy backend`）。
- **版本校验**：就绪行 `protocol_version` 与 SDK 不符立即杀进程报错（含两端版本号）。
- **就绪行格式**：子进程 stdout 单行 JSON `{"agent_store":"listening","host":...,"port":...,"url":...,"protocol_version":...,"version":...,"auth":...}`；SDK 逐行扫描、忽略其他行（tracing 也走 stdout）。

### 5.5 错误与清理

- spawn 失败：报错附 **stderr 尾部 50 行**（`stderr tail:` 段）。
- 超时：默认 120s 后抛 `timed out waiting for the runtime readiness line`。
- 任何失败路径都会 `child.kill()` → 2s 宽限 → `SIGKILL`，并删除自动创建的 data-dir。
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

## 7. 下一步

- 协议方法语义全集：见仓库 `docs/agent-store/05-allo-app-server-protocol.md`。
- 包实现与测试样例：`web/packages/{protocol,client,sdk}/src`（SDK 含 `spawn.test.ts`、`readiness.test.ts` 用例）。
- 浏览器专属辅助（资产 `<img>` URL、`/api/fs/browse`）：宿主 app 实现，不在三包内。