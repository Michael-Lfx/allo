/**
 * `@flowy-agent-store/sdk` — spawn the runtime, connect over loopback, hand back
 * a ready `AppServerClient`.
 *
 * The object `launchHarness` resolves to **is** that client (doc `31`): the
 * `conversations` / `store` / `agents` surface sits on it directly, and the
 * process it owns is reachable as `server` / `handshake` / `close()`. That is
 * the shape both external references use — Codex's
 * `with Codex(...) as codex: codex.thread_start(...)` and Kimi Code's
 * `const harness = createKimiHarness(...); harness.createSession(...)` — the
 * factory returns the main object, lifecycle included, with no `.client` hop.
 */
import {
  AppServerClient,
  WebSocketTransport,
  isLoopbackUrl,
} from "@flowy-agent-store/client";
import type {
  ClientCapabilities,
  ClientInfo,
  InitializeResult,
} from "@flowy-agent-store/protocol";
import { resolveAppServerBin } from "./bin";
import {
  exportAgent,
  exportTeam,
  materializePack,
  type ExportDeps,
  type ExportResult,
} from "./export";
import { parseReadinessLine, type ReadinessInfo } from "./readiness";
import {
  assertProtocolCompatible,
  spawnAppServer,
  type SpawnExitInfo,
  type SpawnOptions,
  type SpawnedServer,
} from "./spawn";

export { resolveAppServerBin };
export { parseReadinessLine, type ReadinessInfo };
export {
  assertProtocolCompatible,
  spawnAppServer,
  type SpawnExitInfo,
  type SpawnOptions,
  type SpawnedServer,
};
export {
  exportAgent,
  exportTeam,
  materializePack,
  type ExportDeps,
  type ExportResult,
};

export interface HarnessOptions extends SpawnOptions {
  client: ClientInfo;
  capabilities?: ClientCapabilities;
  token?: string;
  requestTimeoutMs?: number;
}

/**
 * A connected `AppServerClient` that **owns the runtime process it spawned**
 * (doc `31` §5 方案 B).
 *
 * Every `AppServerClient` member is available directly — `conversations`,
 * `store`, `agents`, `onNotification`, … — because a factory that returned a
 * *wrapper* was the lie this shape removes.
 *
 * Named after the external reference this shape follows (doc `31` §3.2):
 * Kimi Code's `const harness = createKimiHarness(...)` hands back a `harness`
 * you keep for the whole life of the run, not a one-shot launcher.
 */
export interface Harness extends AppServerClient {
  /** The child process: `readiness` / `dataDir` / `exited` / `close`. */
  readonly server: SpawnedServer;
  /**
   * The handshake response captured at launch (**never null**).
   *
   * Not named `initializeResult`: that is a private field of the base class
   * (`client.ts`), and the base's public `initializeInfo` is the *current*
   * connection state — it goes back to `null` after `close()`. This member is
   * the snapshot that proves which protocol fingerprint the session was
   * admitted under.
   */
  readonly handshake: InitializeResult;
  /**
   * Unsubscribe → close the transport → terminate the child → remove an
   * auto-created data dir. Idempotent.
   *
   * Deliberately widens the base's synchronous `close(): void`: awaiting a
   * void-returning call is legal, so callers holding a plain `AppServerClient`
   * are unaffected, while a launched session needs the promise to know the
   * process is actually gone.
   */
  close(): Promise<void>;
}

/**
 * Spawn a loopback runtime with its own data dir and return a connected,
 * initialized client. Rejects non-loopback contact outright (P0-4): this
 * entry point only ever dials the process it just spawned.
 */
export async function launchHarness(options: HarnessOptions): Promise<Harness> {
  const server = await spawnAppServer(options);
  const wsUrl = `ws://${server.readiness.host}:${server.readiness.port}/api/app-server/ws`;
  if (!isLoopbackUrl(wsUrl)) {
    await server.close();
    throw new Error(`refusing to connect outside loopback: ${wsUrl}`);
  }
  const client = new AppServerClient({
    transport: new WebSocketTransport(wsUrl, {
      requestTimeoutMs: options.requestTimeoutMs,
      token: options.token,
    }),
    client: options.client,
    capabilities: options.capabilities,
  });
  // Capture the base method **before** the assignment below shadows it:
  // `client.close` resolves through the own property afterwards, so calling it
  // from inside the new `close` would recurse until the stack blows.
  const closeTransport = client.close.bind(client);
  try {
    const handshake = await client.connect();
    let closed = false;
    return Object.assign(client, {
      server,
      handshake,
      close: async (): Promise<void> => {
        if (closed) return;
        closed = true;
        closeTransport();
        await server.close();
      },
    }) as Harness;
  } catch (error) {
    client.close();
    await server.close();
    throw error;
  }
}
