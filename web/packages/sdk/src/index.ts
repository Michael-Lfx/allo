/**
 * `@flowy-agent-store/sdk` — spawn the runtime, connect over loopback, hand back
 * a ready `AppServerClient`.
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

export interface LaunchOptions extends SpawnOptions {
  client: ClientInfo;
  capabilities?: ClientCapabilities;
  token?: string;
  requestTimeoutMs?: number;
}

export interface LaunchedClient {
  server: SpawnedServer;
  client: AppServerClient;
  initializeResult: InitializeResult;
  /** Disconnect and terminate the spawned runtime. */
  close(): Promise<void>;
}

/**
 * Spawn a loopback runtime with its own data dir and return a connected,
 * initialized client. Rejects non-loopback contact outright (P0-4): this
 * entry point only ever dials the process it just spawned.
 */
export async function launchClient(options: LaunchOptions): Promise<LaunchedClient> {
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
  try {
    const initializeResult = await client.connect();
    let closed = false;
    return {
      server,
      client,
      initializeResult,
      close: async () => {
        if (closed) return;
        closed = true;
        client.close();
        await server.close();
      },
    };
  } catch (error) {
    client.close();
    await server.close();
    throw error;
  }
}
