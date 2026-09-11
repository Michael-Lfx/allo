# TypeScript SDK guide

Flowy Agent Store ships three companion TypeScript packages that let Node.js / Electron / browser applications talk to a local App Server in a type-safe way:

| Package | Responsibility | Runtime | Depends on |
| --- | --- | --- | --- |
| `@flowy-agent-store/protocol` | Wire types (requests/responses/notifications/errors) | Any — zero runtime, no DOM/Node | — |
| `@flowy-agent-store/client` | `AppServerClient` + 7 sub-clients + `Transport` abstraction | Any — no HTTP, no DOM, no Node | `@flowy-agent-store/protocol` |
| `@flowy-agent-store/sdk` | Spawn the `flowy-agent-store` binary → loopback WebSocket → ready client | Node.js (`node:child_process`, …) | `@flowy-agent-store/client`, `@flowy-agent-store/protocol` |

Mix and match: **types only** → `protocol`; **connect to an already-running App Server** (e.g. a desktop app) → `client` with your own `WebSocketTransport`; **launch the whole runtime yourself** → `launchClient` from `sdk`.

> **This page is for developers.** End users do not need it — grab the installer and follow [Quick start](/en-US/docs/quick-start). Two distinct paths: **end users → installer / `install.ps1`**; **developers → npm packages (this page)**.

---

## 1. Install

```bash
# Usually the sdk alone is enough (it re-exports client capabilities and spawns)
bun add @flowy-agent-store/sdk        # or npm install / pnpm add

# Declare protocol explicitly when you import wire types
bun add @flowy-agent-store/protocol
```

All packages ship ESM + CJS (`exports` maps `import` / `require` / `types`); they work out of the box in Node and bundlers.

> **Version status**: all three packages are `0.1.0-beta.*` pre-releases (the API is not frozen, and **no backward compatibility is promised during beta**). Pin an **exact** version in production — this page and the repo currently correspond to `0.1.0-beta.3`. Do not rely on a bare `bun add`: the registry's `latest` currently points at `0.1.0-beta.2`, **not** the newest `0.1.0-beta.3`. For dist-tag semantics, per-version upgrade steps and self-check commands see the [Upgrade and migration guide](/en-US/docs/upgrade).
> **Runtime**: Node.js **≥ 22** (relies on the global `WebSocket`) or Bun; the lower bound is declared by each package's `engines.node`.

---

## 2. Quick start (one-liner with the SDK)

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const session = await launchClient({
  client: { name: "my-app", version: "0.1.0" },
});
const store = await session.client.listStore();
await session.close();
```

What `launchClient` does:

1. Locates the `flowy-agent-store` binary via `bin` → `AGENT_STORE_BIN` → `PATH`;
2. Spawns it with `--host 127.0.0.1 --port 0 --no-open` and an auto-created temp `--data-dir`;
3. Scans stdout for the readiness line (`{"agent_store":"listening",...}`) to learn the actual port;
4. **Validates the readiness `protocol_version` against the SDK** — on mismatch it kills the process and reports both versions;
5. Opens a loopback WebSocket and performs the `initialize` → `initialized` handshake, returning a ready `AppServerClient`.

### Full lifecycle example

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const session = await launchClient({ client: { name: "demo", version: "1.0.0" } });
try {
  // Catalog (Store)
  const items = await session.client.listStore();
  console.log(`${items.items.length} items in the store`);

  // Install and run an agent
  await session.client.installStoreEntry("experts", "frontend-backend-experts");
  const receipt = await session.client.runs.agent({
    agentId: "frontend-backend-experts",
    goal: "Generate a todo REST API",
  });
  const result = await session.client.runs.result(receipt.run_id);
  console.log(result.status);
} finally {
  await session.close(); // terminate child + remove temp data-dir
}
```

---

## 3. `@flowy-agent-store/protocol` — the wire layer

### 3.1 Position

The single TypeScript source of truth for the wire contract: every request/response/notification type, the `APP_SERVER_PROTOCOL_VERSION` constant, and structured errors. **No runtime code at all** — consumable by client, sdk, or anything else speaking the protocol.

### 3.2 Main exports

| Export | Meaning |
| --- | --- |
| `APP_SERVER_PROTOCOL_VERSION` | Current protocol version string (e.g. `"2026-08-26"`); used in the handshake and SDK checks |
| `InitializeRequest` / `InitializeResult` | Handshake request/response (incl. `protocol_version`, server info) |
| `ClientInfo` / `ClientCapabilities` | Caller self-description |
| `StoreList` / `StoreInstallResult` | Winget-style unified catalog |
| `AgentSummary` / `AgentDetail` | AgentDefinition catalog views |
| `TeamSummary` / `TeamDetail` | AgentTeamDefinition catalog views |
| `SkillSummary` / `SkillDetail` | Skill catalog views |
| `ConnectorSummary` / `ConnectorDetail` / `ConnectorStatusView` / `ConnectorProbeResult` | Connector catalog / status / probe |
| `OAuthStartResult` / `OAuthStatusView` | OAuth browser-flow state |
| `ConversationView` / `ConversationMessage` / `ConversationEvent` / `ConversationSendReceipt` | Persistent conversations |
| `RunReceipt` / `RunView` / `RunResult` / `RunEvent` | Run lifecycle |
| `JsonRpcRequest` / `JsonRpcResponse` / `JsonRpcNotification` | Wire frame types |
| `ServerNotification` | Server notifications (`event`, `conversation/event`, `run/resync-required`, …) |
| `WireError` | Server error payload |

> Experimental capabilities (full Team collaboration, event cursor catch-up) stay marked `experimental` and are excluded from stable exports.

### 3.3 Error model (`errors.ts`)

**Branch on the stable `code`, never parse the human-readable message:**

| Class | Trigger | Key fields |
| --- | --- | --- |
| `AppServerError` | Server returned a JSON-RPC error | `code`, `request_id`, `retryable`, `details` |
| `TransportError` | Transport layer (connect/send/close) | `phase` (`connect`/`send`/`receive`/`close`), `retryable` |
| `ProtocolError` | Local protocol validation failed | `kind` (`invalid_message` / `version_mismatch` / `unexpected_response`) |
| `RequestTimeoutError` | Request timed out | `method`, `timeoutMs` |

Helpers:

```ts
import { isAppServerError, isRetryableTransportError, formatError } from "@flowy-agent-store/protocol";

try {
  await client.runs.agent({ agentId, goal });
} catch (error) {
  if (isAppServerError(error)) {
    // stable code (e.g. version_mismatch / marketplace_not_found) — not the message
    console.log(error.code, error.retryable);
  } else if (isRetryableTransportError(error)) {
    // connection dropped, safe to retry
  }
  console.log(formatError(error)); // the one shared UI rendering
}
```

> Idempotency conflicts and policy denials are **never auto-retried** (`retryable: false`) — replaying them stacks side effects.

---

## 4. `@flowy-agent-store/client` — the transport-agnostic client

### 4.1 Position

Pure business layer: every method goes through the injected `Transport`. No HTTP, no DOM, no Node in the package. The connection lifecycle (`connect → initialize → version check → initialized → ready`) lives here, so business code never knows whether the channel is WebSocket, stdio, or a future one-shot HTTP binding.

### 4.2 The `Transport` interface

```ts
export interface Transport {
  connect(): Promise<void>;                          // open the channel (idempotent)
  request<T>(method: string, params: unknown): Promise<T>;  // request-response
  notify(method: string, params: unknown): void;     // fire-and-forget
  onNotification(listener: NotificationListener): () => void; // subscribe; returns unsubscribe
  close(): void;
  onLifecycle?(listener: (state: "open" | "closed") => void): () => void; // optional channel lifecycle
}
```

Built-in `WebSocketTransport` (browser + Node 22+/Bun, uses the global `WebSocket`):

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport(
  "ws://127.0.0.1:8787/api/app-server/ws",
  { requestTimeoutMs: 30_000, connectTimeoutMs: 10_000, token: "optional-bearer" } // token becomes ?token= in browsers
);
const client = new AppServerClient({ transport, client: { name: "my-app", version: "0.1.0" } });
await client.connect(); // initialize handshake + version check
```

`WebSocketTransport` implementation contract (A2 / T8):

- **`connect()` is idempotent and concurrency-safe**: concurrent calls share one dial (no second socket is opened); after `connectTimeoutMs` (default 10s) without opening it rejects `TransportError(phase: "connect", retryable: true)` and closes that socket.
- **`close()` is terminal cleanup**: it settles every pending request and any in-flight `connect()` (`TransportError(phase: "close")`) and clears the listeners registered through `onNotification`. So **after closing, a new `connect()` must re-register its listeners** (`AppServerClient` re-arms its notification bridge automatically; custom transports must do it themselves).
- **Stale-connection isolation**: events arriving late from a replaced or closed socket are ignored and never touch the current connection.
- **No auto-reconnect**: a dropped connection only fails pending requests with `TransportError(retryable: true)`; the reconnect policy belongs to the caller.
- **Reconnect is observable (T8)**: `onLifecycle` reports `open` after every successful dial, and `closed` only when an **established** connection is lost (a failed first dial is not a disconnect, and neither is a caller-initiated `close()`). `closed → open` is one reconnect. These listeners **survive `close()`** — they exist to drive the reconnection.
- **After a reconnect you must re-handshake and re-subscribe**: a new socket means the server-side subscriptions and the local `onNotification` listeners are gone, so the flow is `transport onLifecycle("open")` → `client.connect()` (re-runs the `initialize` handshake) → `rearm()` every live subscription.

Bring your own transport by implementing the interface: an in-memory fake for tests, stdio for CLIs, Node WebSocket in Electron main — business code does not change.

### 4.3 `AppServerClient` top-level methods

| Method | Wire method | Meaning |
| --- | --- | --- |
| `connect()` | `initialize` + `initialized` | Handshake; after success `ready === true`. Version mismatch throws `ProtocolError(version_mismatch)` |
| `close()` | — | Close the transport; the server revokes the connection immediately |
| `onNotification(listener)` | — | Global notification subscription (returns unsubscribe) |
| `ready` / `initializeInfo` | — | Whether ready / handshake result |
| `runImport(input)` · `listImports()` · `getImport(snapshotId)` | `import/*` | Import local CodeBuddy/WorkBuddy dirs |
| `runInstall(input)` · `getInstallStatus(snapshotId)` | `install/run` / `install/status` | Snapshot install |
| `disableInstall(snapshotId, ids)` · `enableInstall(...)` · `uninstallInstall(...)` | `install/*` | Component enable/disable/uninstall |
| `addMarketplace(input)` · `listMarketplaces()` · `getMarketplace(id)` | `market/*` | Marketplace source management |
| `removeMarketplace(id, cascade)` | `market/remove` | `cascade=true` uninstalls snapshots installed from it |
| `setMarketplaceAutoUpdate(id, enabled)` | `market/auto-update` | Auto-update toggle (DB flag) |
| `refreshMarketplace(id)` | `market/refresh` | Re-fetch source, rebuild entries when revision changed |
| `importMarketplaceEntry(mkt, entry)` | `market/entry-import` | Import one entry (provenance-linked) |
| `listStore()` | `store/list` | Unified catalog across marketplaces (with install state) |
| `installStoreEntry(mkt, entry)` | `store/install-entry` | One-click install: import (if missing) + register |

> ℹ️ **The first `listStore()` may come back empty or partial — by design, not an error**: the built-in default marketplaces register in the **background** (D-SDK-1 ①). The first `store/list` only kicks that off and then answers from whatever is registered *right now*; it never waits for the mirror.
> Registration itself HTTP-mirrors each market source's whole tree (hundreds of skill dirs, thousands of assets), roughly **90s** on a fresh data dir — during which that call reports `items: 0`.
> Measured (2026-09-10, fresh local data dir): `first store/list: 1ms items=0` → after 130s `store/list: 133ms items=438`, `market/list count=3`.
> So **you do not need a larger `requestTimeoutMs` for the first call**; re-list after the warm-up for the full catalog (the WebUI has an explicit refresh). If a mirror is unreachable, that warm-up counts as incomplete and the next store/market call retries automatically.
> To tell "no markets at all" from "still loading": `listStore()` (that is, `store/list`) now returns `markets_pending` — `true` means the builtin markets are still registering in the background and the catalog may be incomplete.
> If you own a fixed `dataDir`, later calls on it short-circuit idempotently and do no network I/O.

### 4.4 Sub-clients

All constructed on the same transport; every method returns `Promise<T>`.

#### `agents` — AgentDefinition catalog

```ts
client.agents.list(): Promise<AgentSummary[]>;
client.agents.get(agentId: string): Promise<AgentDetail>;
```

#### `teams` — AgentTeamDefinition catalog

```ts
client.teams.list(): Promise<TeamSummary[]>;
client.teams.get(teamId: string): Promise<TeamDetail>;
```

#### `skills` — Skill catalog

```ts
client.skills.list(): Promise<SkillSummary[]>;
client.skills.get(skillId: string): Promise<SkillDetail>;
```

#### `connectors` — catalog / status / OAuth

```ts
client.connectors.list(): Promise<ConnectorSummary[]>;
client.connectors.get(connectorId: string): Promise<ConnectorDetail>;        // namespaced tools + auth state
client.connectors.status(connectorId: string): Promise<ConnectorStatusView>; // connected only when auth ready AND last probe OK
client.connectors.test(connectorId: string): Promise<ConnectorProbeResult>;  // run probe (result persisted)
client.connectors.authStatus(connectorId: string): Promise<OAuthStatusView>;
client.connectors.authStart(connectorId: string): Promise<OAuthStartResult>; // start host browser OAuth flow; poll authStatus until authenticated
client.connectors.logout(connectorId: string): Promise<void>;                // revoke token
```

> Tokens never pass through this package: the OAuth browser flow is owned by the trusted host; clients only trigger and poll.

#### `conversations` — persistent conversations

```ts
client.conversations.create(input): Promise<ConversationView>;  // omit model → server resolves the default from config.toml
client.conversations.update(id, input): Promise<ConversationView>;
client.conversations.modelOptions(): Promise<ConversationModelOptions>;
client.conversations.list(limit = 100): Promise<ConversationView[]>;
client.conversations.get(id): Promise<ConversationView>;
client.conversations.messages(query): Promise<ConversationMessagesPage>; // page/page_size/cursor
client.conversations.send(id, content, idempotencyKey): Promise<ConversationSendReceipt>; // explicit idempotency key required
client.conversations.cancel(id): Promise<ConversationView>;
client.conversations.delete(id): Promise<{ conversation_id: string; deleted: boolean }>;
await client.conversations.follow(id): Promise<ConversationSubscription>;
```

Every entry in `modelOptions()` carries `name` / `display_name` / `context_limit` and may additionally carry **models.dev catalog facts**: `cost_input` / `cost_output` (USD per million tokens), `catalog_context_window`, and `supports_vision`. When the registry has no entry for that provider+model the fields are **absent entirely** — read them as "unknown", never as `false` or `0`.

Live subscription object:

```ts
const sub = await client.conversations.follow(convId);
sub.onEvent((event) => console.log("seq", event.sequence, event)); // deduped by sequence
sub.onResync((reason) => console.log("resync required:", reason)); // catch-up hint after disconnects
sub.lastSequence; // highest sequence seen
await sub.rearm(); // after a reconnect: re-register + reset cursor + re-issue conversation/subscribe
await sub.close(); // server side unsubscribe (closing the socket also works)
```

> After `rearm()` you still have to backfill the outage window yourself: this subscription has no event-replay API, so re-fetch with `conversation/messages`. The reset cursor means later duplicates are the caller's to dedupe by `sequence`.

#### `runs` — run lifecycle and live events

```ts
client.runs.agent(input: AgentRunInput): Promise<RunReceipt>; // async receipt, not the final result
client.runs.get(runId): Promise<RunView>;                     // authoritative state
client.runs.result(runId): Promise<RunResult>;                // resolves only at a terminal state
client.runs.events({ runId, afterSequence, limit }): Promise<RunEvent[]>; // cursor replay
client.runs.cancel({ runId, expectedVersion, commandId, idempotencyKey }): Promise<RunView>;
await client.runs.follow(runId): Promise<EventSubscription>;
```

```ts
const sub = await client.runs.follow(runId);
sub.onEvent((event) => console.log(event));       // best-effort events (lossy, unordered)
sub.onResync(({ run_ids, reason }) => …);         // subscription invalidated, replay required
sub.onError((error) => …);                        // transport errors forwarded
sub.lastSequence;
const replayed = await sub.rearm();               // after a reconnect: reset cursor + re-subscribe + replay all
await sub.close();
```

> `rearm()` **replays the whole history** (the cursor reset is deliberate), so consumers must dedupe by `sequence`; its return value is the replayed batch. Call it only after the reconnect handshake (`initialize`) has completed.

> Event delivery is best-effort: durability relies on `run/events` cursor replay, so Node consumers should dedupe and order themselves.

#### `workspaces` — workspace registration

```ts
client.workspaces.list(): Promise<WorkspaceView[]>;
client.workspaces.create(path: string): Promise<WorkspaceView>; // server canonicalizes, rejects links/reparse points
client.workspaces.revoke(workspaceId: string): Promise<WorkspaceRevokeResult>; // soft delete; conversations kept
```

---

## 5. `@flowy-agent-store/sdk` — the Node host

### 5.1 `launchClient(options): Promise<LaunchedClient>`

```ts
interface LaunchOptions extends SpawnOptions {
  client: ClientInfo;             // { name, version }
  capabilities?: ClientCapabilities;
  token?: string;                 // handed to WebSocketTransport
  requestTimeoutMs?: number;      // default 30s (not enough for the first store/list — see the §4.3 warning)
}

interface LaunchedClient {
  server: SpawnedServer;          // readiness / dataDir / exited / close
  client: AppServerClient;        // already connected + initialized
  initializeResult: InitializeResult;
  close(): Promise<void>;         // unsubscribe → close transport → kill child → remove temp data-dir
}
```

### 5.2 Lower-level primitives

| Export | Meaning |
| --- | --- |
| `spawnAppServer(options: SpawnOptions)` | Spawn + wait for readiness only (no connect). `SpawnOptions` below |
| `resolveAppServerBin(explicit?)` | Locate the binary (§5.3) |
| `parseReadinessLine(line)` | Parse one line; `null` when not the readiness line |
| `ReadinessInfo` | `{ host, port, url, protocol_version, version, auth }` |
| `assertProtocolCompatible(runtimeVersion)` | Throws on mismatch (both versions in the message) |
| `SpawnExitInfo` | `{ code, signal }` — how the child exited (payload of `exited` / `onExit`) |

`SpawnOptions`:

```ts
interface SpawnOptions {
  bin?: string;            // explicit path (overrides everything)
  dataDir?: string;        // your own dir ⇒ you own it; omitted ⇒ temp dir removed on close
  port?: number;           // default 0 = OS-assigned
  extraArgs?: string[];    // extra CLI args appended after managed ones
  readyTimeoutMs?: number; // default 120s (cold DB init)
  env?: Record<string, string | undefined>; // merged over process.env
  cwd?: string;            // child working directory; omitted ⇒ inherits the parent's
  onExit?: (info: SpawnExitInfo) => void;   // called once when the child exits
}
```

`SpawnedServer.exited` is a `Promise<SpawnExitInfo>` that **never rejects**: it settles whenever the child ends, for any reason — the only entry point for observing a runtime crash.

### 5.3 Binary resolution

Order: `bin` argument → env `AGENT_STORE_BIN` → `flowy-agent-store` / `flowy-agent-store.exe` on `PATH`. Not found ⇒ **hard error; never downloads or guesses** (release-asset download is P2).

```bash
AGENT_STORE_BIN=/opt/flowy-agent-store/flowy-agent-store node your-app.mjs
```

### 5.4 Runtime contract (P0, verified)

- **Loopback enforced**: the child always runs `--host 127.0.0.1 --no-open`; the SDK only ever dials the process it spawned (`isLoopbackUrl` rejects anything else before connecting).
- **Data-dir exclusivity**: omit `dataDir` ⇒ auto `mkdtemp`, removed on `close()`; passing your own dir means you own it — the backend single-instance lock fails fast (`already in use by another running Flowy backend`).
- **Version check**: readiness `protocol_version` mismatch kills the child and reports both versions.
- **Readiness line**: a single stdout JSON line `{"agent_store":"listening","host":...,"port":...,"url":...,"protocol_version":...,"version":...,"auth":...}`; the SDK scans lines and ignores everything else (tracing shares stdout).
- **stdout kept drained**: once the readiness line is parsed the SDK keeps reading and discarding the child's stdout (`readline.close()` pauses that stream, so reading must not stop there). Otherwise the runtime blocks forever once its logs fill the OS pipe buffer (~64KB) — long sessions (multi-turn runs, market-tree scans) then hang silently. Post-readiness output is only drained and dropped; this release exposes no log callback.
- **`env` / `cwd` passthrough**: `env` is **merged over** the parent's `process.env` (not a replacement, so `PATH` etc. stay visible); omitting `cwd` inherits the parent working directory. Both go to `child_process.spawn` unchanged.
- **Exit is observable**: `SpawnedServer.exited` (`{ code, signal }`) settles whenever the child ends, for **any** reason including a crash or a non-zero code, and `onExit` fires once alongside it. The SDK **never restarts** the runtime; restarting belongs to the caller of `launchClient`.

### 5.5 Errors and cleanup

- Spawn failure: the error appends the **last 50 stderr lines** (`stderr tail:` section).
- Timeout: after the default 120s it throws `timed out waiting for the runtime readiness line`.
- Every failure path runs `child.kill()` → 2s grace → `SIGKILL`, and removes the auto-created data dir.
- After readiness the promise is already settled: a later `exit` / `error` from the child no longer takes the failure path (it is not reported as a startup failure); such exits (crashes included) surface only through `SpawnedServer.exited` and `onExit`. The SDK never restarts the runtime, and lifetime is owned by the caller via `close()`.
- Correct usage: `close()` in a `try/finally`; without it the temp dir leaks on process exit (no exit hook installed).

```ts
const session = await launchClient({ client: { name: "x", version: "1" } });
try {
  await session.client.connectors.list();
} finally {
  await session.close();
}
```

---

## 6. Full scenario: browser talking to the desktop app

When a desktop App Server is already running, the browser connects directly (no sdk):

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const ws = new WebSocketTransport(`ws://127.0.0.1:8787/api/app-server/ws`);
const client = new AppServerClient({ transport: ws, client: { name: "web", version: "1.0.0" } });
await client.connect();
console.log("connected:", client.ready);
```

> Browser WebSockets cannot set custom headers: `WebSocketTransport` appends `token` as `?token=`; Node-side HTTP uses the `Authorization` header.

---

## 7. Per-method API reference

How the three packages' real exports line up with the protocol methods. Method names follow `05`; this table introduces no new ones.

### 7.1 `AppServerClient` top-level methods

| Method | Params | Returns | Protocol method |
| --- | --- | --- | --- |
| `connect()` | — | `InitializeResult` | `initialize` → `initialized` |
| `onNotification(listener)` | `(notification) => void` | unsubscribe | — (server notifications) |
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

### 7.2 Sub-clients

| Sub-client | Methods | Protocol methods |
| --- | --- | --- |
| `agents` | `list()` / `get(agentId)` | `agent/list` / `agent/get` |
| `teams` | `list()` / `get(teamId)` | `team/list` / `team/get` |
| `skills` | `list()` / `get(skillId)` | `skill/list` / `skill/get` |
| `connectors` | `list()` / `get(id)` / `status(id)` / `test(id)` / `authStatus(id)` / `authStart(id)` / `logout(id)` | `connector/list` · `get` · `status` · `test` · `auth/status` · `auth/start` · `auth/logout` |
| `conversations` | `create(input)` / `update(id, input)` / `modelOptions()` / `list(limit?)` / `get(id)` / `messages(query)` / `send(id, content, idempotencyKey)` / `cancel(id)` / `delete(id)` / `follow(id, options?)` | the same-named `conversation/*` methods |
| `runs` | `agent(input)` / `get(id)` / `result(id)` / `events(query)` / `cancel(input)` / `steer(input)` / `answerDecision(input)` / `follow(id, options?)` | `agent/run` · `run/get` · `run/result` · `run/events` · `run/cancel` · `run/steer` · `run/answer-decision` |
| `workspaces` | `list()` / `create(path)` / `revoke(id)` | `workspace/list` / `workspace/create` / `workspace/revoke` |
| `models` | `list()` | `models/list` |

### 7.3 HTTP binding

HTTP and WebSocket are two bindings of one method semantics. `httpRouteTable()` returns the **machine-readable route table** (method → verb + path + provenance), so this guide does not hand-copy it:

```ts
import { httpRouteTable } from "@flowy-agent-store/client";

const routes = httpRouteTable();
// { "market/remove": { verb: "POST", path: "/markets/:marketplace_id/remove", source: "…" }, … }
```

- Covers **45 / 64** methods. The 19 without an HTTP binding: `initialize`, `initialized`, `workspace/create`, `conversation/model-options`, `conversation/update`, `conversation/subscribe`, `conversation/unsubscribe`, `run/subscribe`, `run/unsubscribe`, `agent/list`, `agent/get`, `team/list`, `team/get`, `config/get`, `config/set`, `skill/create`, `skill/update`, `skill/delete`, `skill/copy`.
- `config/get` / `config/set` (the host settings file `~/.agent-store/config.toml`) are **host management surface** (`16` §6): wire methods with no HTTP binding, and deliberately **not part of this package's client** — the Web UI calls them through its own transport helpers. Contract in `05` §4.10.
- `skill/create` / `skill/update` / `skill/delete` / `skill/copy` (the skill write face, `16` R17 / W12) are host management surface by the same `16` §6 judgement: a third-party consumer must not be able to write files into the host's skill tree, so they are wire-only, have no HTTP binding, and are not in this package. `skill/update` is a **field-level patch** (only the named fields move; `name` is not editable) and `skill/copy` derives a writable user skill from any origin. The read face's `SkillSummary` gains `origin` / `writable` (additive); contract in `05` §4.11.
- **`HttpTransport` is the request/response binding and is not equivalent to WebSocket**: `notify()` throws and `onNotification()` returns a no-op unsubscribe. Live events and subscriptions require `WebSocketTransport`.
- Every call performs its own handshake (`initialize` → `initialized` → business call), so `connect()` is a no-op. Host-side code that needs a ready connection id calls `openConnection()`.
- `/api/fs/*` (browse / list / read / metadata) is a host file service, not a protocol method, and is not part of this package.

### 7.4 Answering approvals: `run/answer-decision`

A run stops when it needs a human decision: `run/events` projects `approval.requested`, and the answer goes through `runs.answerDecision(input)`:

```ts
const pending = (await client.runs.events({ runId })).find(
  (event) => event.event_type === "approval.requested",
);

await client.runs.answerDecision({
  runId,
  stepId: pending.step_id!,                    // attempt scope, projected on the event
  attemptId: pending.attempt_id!,
  answer: "Approved, continue",
  expectedExecutionVersion: pending.expected_execution_version!,  // the three CAS versions
  expectedStepVersion: pending.expected_step_version!,
  expectedAttemptVersion: pending.expected_attempt_version!,
});
```

- **The three `expected*Version` values are mandatory CAS tokens**, not an optional nicety: the server passes them straight to the engine's single answer gate, and any one of them having moved returns `conflict` instead of silently overwriting. `run/events` projects the current three onto every unanswered `approval.requested` (read from the authoritative rows at projection time), so a client echoes them rather than inventing versions.
- **Only a `waiting_input` attempt can be answered**; a foreign owner, a stale version, a non-waiting attempt, or an empty answer are all refused (`NotFound` / `Conflict` / `BadRequest`).
- **There is no `always_allow`**: the desktop confirmation route's approve-all switch is not part of this protocol. The params are `deny_unknown_fields`, so sending it fails with `invalid_request`.
- `RunEvent.step_id` / `attempt_id` are present only when the engine scoped the event to an attempt (typically `approval.requested` / `approval.responded`).

## 8. Event reference: `sequence` and catch-up

### 8.1 Event types

`ConversationEventType` is a **closed union** (`protocol.ts`) with 9 members:

| Event type | Meaning | Decoded kind |
| --- | --- | --- |
| `message.created` | A message was persisted | `message.created` |
| `message.delta` | Body increment (`replace` swaps wholesale) | `message.delta` |
| `message.thinking` | Thinking increment | `message.thinking` |
| `message.tips` | Tip row (`tip_type`) | `message.tips` |
| `message.tool` | Tool call (streams running → completed) | `message.tool` |
| `message.error` | Terminal error (decoded with `code` and `retryable`) | `message.error` |
| `message.activity` | Activity row (`kind` drives rendering; carries this turn's token usage when `kind === "turn_completed"`) | `message.activity` |
| `turn.status` | Turn busy/idle (`status === "running"`) | `turn.status` |
| `context.usage` | Context usage | `context.usage` |

The server spells "thinking" two ways: `message.thinking`, and `message.activity` with `kind === "thinking"`. `decodeConversationEvent` **normalises** the latter into `message.thinking`, so callers keep one thinking path. Unknown types land in `unknown` (raw `event_type` preserved) rather than being misread as a known one.

A `message.activity` frame whose `kind === "turn_completed"` also carries **this turn's** token usage (`usage: { input_tokens, output_tokens, total_tokens }`, the runtime's per-turn report). The field decodes to `null` when the runtime reported nothing, only one side, or two zeros — **"unknown" is not "free"**, so callers must not substitute context occupancy or a zero. Per-turn usage arrives only on the live stream (the server does not persist past turns), and the field names are snake_case, matching the Run-side `TurnUsage`.

`message.error` decodes `code` (the server's error code) and `retryable` alongside the message text. `retryable` is **three-valued**: `true`, `false`, or `null` — `null` means the wire did not supply it (history rows, commonly), and callers must not guess it into `false` or `true`. Re-reading the send receipt also exposes `result_error_retryable`, which agrees with it.

### 8.2 `sequence` semantics

- `sequence` is a **per-conversation, monotonic and contiguous** counter (the server keeps one per conversation). It is not a global ordinal.
- Unsubscribing destroys that counter; resubscribing starts at `1`, which is why `rearm()` resets the local cursor to `0`.
- Gap detection: `sequence > lastSeen + 1` while `lastSeen > 0` means loss — the subscription emits `onResync("gap")` and triggers catch-up.
- Duplicates and out-of-order frames (`sequence <= lastSeen`) are dropped and never re-delivered.

### 8.3 Catch-up

Conversations and runs catch up through different carriers:

| Case | Server signal | Catch-up mechanism | Package entry point |
| --- | --- | --- | --- |
| Conversation | `conversation/resync-required` | re-fetch `conversation/messages` (V1 has no conversation event replay) | `follow(..., { fetchMessages })` → `onBackfill` |
| Run | `run/resync-required` | replay `run/events` with `after_sequence` | `follow()` auto-resyncs; `resync()` / `catchUp()` for manual |

Conversation subscriptions auto-catch-up by default (`autoResync`) and run at most one fetch at a time (bursts coalesce); the fetched page is handed to `onBackfill`. If your layer owns the pagination cursor, pass `autoResync: false` and listen only to `onResync`, then reload authoritatively yourself.

On a `client` that has already `connect()`ed (full setup in §10):

```ts
const subscription = await client.conversations.follow(conversationId);
subscription.onEvent((event) => {
  const decoded = decodeConversationEvent(event);
  if (decoded.kind === "message.delta") render(decoded.delta, decoded.replace);
});
subscription.onBackfill((snapshot) => resetTranscript(snapshot.messages));
subscription.onError((error) => report(error));
```

## 9. Error model and retry

All four error classes are exported from `@flowy-agent-store/protocol`; `retryable` is the stable contract (never branch on `message`):

| Class | Raised when | `retryable` |
| --- | --- | --- |
| `AppServerError` | the server returned a business error; carries `code` / `request_id` / `details` | per the server hint |
| `TransportError` | connect / send / receive / close failed; carries `phase` | decided by `phase` and the caller |
| `ProtocolError` | malformed message, version mismatch, unexpected response; carries `kind` | no |
| `RequestTimeoutError` | request timed out; carries `method` / `timeoutMs` | no |

`isRetryableError(error)` is the single retry predicate; `formatError(error)` is the single human-readable rendering. `withRetry(operation, options)` backs off exponentially (with jitter) on `retryable`:

| Option | Default | Meaning |
| --- | --- | --- |
| `maxAttempts` | `3` | total attempts including the first |
| `baseDelayMs` | `500` | first backoff |
| `maxDelayMs` | `8000` | ceiling for one delay |
| `jitter` | `0.25` | jitter fraction; delay lands in `[0.75×, 1.0×]` |
| `onRetry` | — | called before each retry with `{ attempt, delayMs, error }` |
| `shouldRetry` | protocol `retryable` | custom predicate |
| `sleep` | `setTimeout` | injectable (tests) |

```ts
import { withRetry } from "@flowy-agent-store/client";

const view = await withRetry(() => client.runs.get(runId), {
  maxAttempts: 4,
  onRetry: ({ attempt, delayMs }) => log(`retry ${attempt} in ${delayMs}ms`),
});
```

Writes carrying an `idempotency_key` / `command_id` replay safely: the App Server deduplicates same-key requests instead of executing twice.

## 10. Examples: Node / browser / Electron

### 10.1 Node: launch a local runtime in one call

```ts
import { launchClient } from "@flowy-agent-store/sdk";

const launched = await launchClient({ client: { name: "my-tool", version: "1.0.0" } });
try {
  const conversation = await launched.client.conversations.create({ name: "demo" });
  const subscription = await launched.client.conversations.follow(conversation.conversation_id);
  subscription.onEvent((event) => console.log(event.event_type));
  await launched.client.conversations.send(conversation.conversation_id, "hello", crypto.randomUUID());
} finally {
  await launched.close();
}
```

### 10.2 Browser: connect to an already-running server

The browser spawns nothing and only connects over WebSocket; `WebSocketTransport` passes credentials as `?token=` (browser WebSocket cannot set custom headers).

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport("ws://127.0.0.1:8787/api/app-server/ws", { token });
const client = new AppServerClient({ transport, client: { name: "web", version: "1.0.0" } });
await client.connect();
```

### 10.3 Electron: spawn in the main process, connect from the renderer

The main process owns the binary and the data dir; the renderer only receives a loopback URL and token. Keep credentials in the main process (OS credential store), never in the renderer or in plaintext config.

```ts
import { spawnAppServer } from "@flowy-agent-store/sdk";

const server = await spawnAppServer({ dataDir: app.getPath("userData") });
win.webContents.send("app-server-ready", {
  url: `ws://${server.readiness.host}:${server.readiness.port}/api/app-server/ws`,
});
app.on("before-quit", () => void server.close());
```

For pure request/response work (no live events) use `HttpTransport`; it subscribes to nothing and handshakes per call.

## 11. MCP integration guide

The official MCP path for Agent Store is the **connector descriptor**, with no second format introduced:

| Case | Where it is declared |
| --- | --- |
| Connector market entry | an entry in `.codebuddy-connector/connectors.json` |
| MCP servers shipped by a plugin | the plugin manifest's `mcpServers` field |

Remote HTTP/SSE and local stdio servers are both described verbatim in the manifest; Agent Store only hosts them and proxies tools under a namespace — it never executes connector content. Credentials:

- Mark sensitive config entries in the `userConfig` schema; values go to the OS credential store.
- Do not put secrets in `env`: the current version writes `env` into the snapshot in plaintext (a registered known deviation). Use `userConfig` until that is fixed.

More manifest fields and examples: [Plugins and market](/en-US/docs/plugins-market).

## 12. Next steps


- Full method semantics: repo `docs/agent-store/05-allo-app-server-protocol.md`.
- Implementation and test samples: `web/packages/{protocol,client,sdk}/src` (the sdk has `spawn.test.ts`, `readiness.test.ts`).
- Browser-only helpers (asset `<img>` URLs, `/api/fs/browse`): implemented by the host app, not in these three packages.