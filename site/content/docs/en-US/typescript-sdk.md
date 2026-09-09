# TypeScript SDK guide

Flowy Agent Store ships three companion TypeScript packages that let Node.js / Electron / browser applications talk to a local App Server in a type-safe way:

| Package | Responsibility | Runtime | Depends on |
| --- | --- | --- | --- |
| `@flowy-agent-store/protocol` | Wire types (requests/responses/notifications/errors) | Any — zero runtime, no DOM/Node | — |
| `@flowy-agent-store/client` | `AppServerClient` + 7 sub-clients + `Transport` abstraction | Any — no HTTP, no DOM, no Node | `@flowy-agent-store/protocol` |
| `@flowy-agent-store/sdk` | Spawn the `flowy-agent-store` binary → loopback WebSocket → ready client | Node.js (`node:child_process`, …) | `@flowy-agent-store/client`, `@flowy-agent-store/protocol` |

Mix and match: **types only** → `protocol`; **connect to an already-running App Server** (e.g. a desktop app) → `client` with your own `WebSocketTransport`; **launch the whole runtime yourself** → `launchClient` from `sdk`.

---

## 1. Install

```bash
# Usually the sdk alone is enough (it re-exports client capabilities and spawns)
bun add @flowy-agent-store/sdk        # or npm install / pnpm add

# Declare protocol explicitly when you import wire types
bun add @flowy-agent-store/protocol
```

All packages ship ESM + CJS (`exports` maps `import` / `require` / `types`); they work out of the box in Node and bundlers.

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
}
```

Built-in `WebSocketTransport` (browser + Node 22+/Bun, uses the global `WebSocket`):

```ts
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";

const transport = new WebSocketTransport(
  "ws://127.0.0.1:8787/api/app-server/ws",
  { requestTimeoutMs: 30_000, token: "optional-bearer" } // token becomes ?token= in browsers
);
const client = new AppServerClient({ transport, client: { name: "my-app", version: "0.1.0" } });
await client.connect(); // initialize handshake + version check
```

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

Live subscription object:

```ts
const sub = await client.conversations.follow(convId);
sub.onEvent((event) => console.log("seq", event.sequence, event)); // deduped by sequence
sub.onResync((reason) => console.log("resync required:", reason)); // catch-up hint after disconnects
sub.lastSequence; // highest sequence seen
await sub.close(); // server side unsubscribe (closing the socket also works)
```

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
await sub.close();
```

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
  requestTimeoutMs?: number;      // default 30s
}

interface LaunchedClient {
  server: SpawnedServer;          // readiness/dataDir/close
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

`SpawnOptions`:

```ts
interface SpawnOptions {
  bin?: string;            // explicit path (overrides everything)
  dataDir?: string;        // your own dir ⇒ you own it; omitted ⇒ temp dir removed on close
  port?: number;           // default 0 = OS-assigned
  extraArgs?: string[];    // extra CLI args appended after managed ones
  readyTimeoutMs?: number; // default 120s (cold DB init)
}
```

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

### 5.5 Errors and cleanup

- Spawn failure: the error appends the **last 50 stderr lines** (`stderr tail:` section).
- Timeout: after the default 120s it throws `timed out waiting for the runtime readiness line`.
- Every failure path runs `child.kill()` → 2s grace → `SIGKILL`, and removes the auto-created data dir.
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

## 7. Next steps

- Full method semantics: repo `docs/agent-store/05-allo-app-server-protocol.md`.
- Implementation and test samples: `web/packages/{protocol,client,sdk}/src` (the sdk has `spawn.test.ts`, `readiness.test.ts`).
- Browser-only helpers (asset `<img>` URLs, `/api/fs/browse`): implemented by the host app, not in these three packages.