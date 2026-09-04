/**
 * Spawn the `agent-store` runtime as a child process and wait for its
 * readiness line (docs/agent-store/12 P0-1 / P1-3).
 *
 * The child is always loopback-only (`--host 127.0.0.1 --no-open`) with its
 * own data directory (SDK独占, P0-3): a fresh temp dir by default, or an
 * explicit one the caller owns. A server-side single-instance lock fails the
 * spawn fast when the directory is already held — that error propagates.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { APP_SERVER_PROTOCOL_VERSION } from "@agent-store/protocol";
import { resolveAppServerBin } from "./bin";
import { parseReadinessLine, type ReadinessInfo } from "./readiness";

const DEFAULT_READY_TIMEOUT_MS = 120_000;
const STDERR_TAIL_LINES = 50;
const KILL_GRACE_MS = 2_000;

/** Fail when the runtime speaks a different wire protocol (§7 acceptance). */
export function assertProtocolCompatible(runtimeVersion: string): void {
  if (runtimeVersion !== APP_SERVER_PROTOCOL_VERSION) {
    throw new Error(
      `protocol version mismatch: runtime speaks ${runtimeVersion}, SDK expects ${APP_SERVER_PROTOCOL_VERSION}`,
    );
  }
}

export interface SpawnOptions {
  /** Explicit binary path (overrides `AGENT_STORE_BIN` / `PATH`). */
  bin?: string;
  /** Data directory the child owns. Defaults to a fresh temp dir (removed on `close`). */
  dataDir?: string;
  /** Port to bind. Defaults to `0` (OS-assigned, reported by the readiness line). */
  port?: number;
  /** Extra CLI args appended after the managed ones. */
  extraArgs?: string[];
  /** How long to wait for the readiness line. Defaults to 120s (cold DB init). */
  readyTimeoutMs?: number;
}

export interface SpawnedServer {
  readonly readiness: ReadinessInfo;
  readonly dataDir: string;
  /** Terminate the child and remove the data dir when it was auto-created. */
  close(): Promise<void>;
}

export async function spawnAppServer(options: SpawnOptions = {}): Promise<SpawnedServer> {
  const bin = resolveAppServerBin(options.bin);
  const ownedDir = options.dataDir === undefined;
  const dataDir = ownedDir ? await mkdtemp(join(tmpdir(), "agent-store-sdk-")) : options.dataDir as string;
  const port = options.port ?? 0;

  const child = spawn(bin, ["--host", "127.0.0.1", "--port", String(port), "--data-dir", dataDir, "--no-open", ...(options.extraArgs ?? [])], {
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });

  const stderrTail: string[] = [];
  child.stderr?.on("data", (chunk: Buffer) => {
    for (const line of String(chunk).split(/\r?\n/)) {
      if (line.length === 0) continue;
      stderrTail.push(line);
      if (stderrTail.length > STDERR_TAIL_LINES) stderrTail.shift();
    }
  });

  const stopChild = async (): Promise<void> => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill();
    const exited = await new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => resolve(false), KILL_GRACE_MS);
      child.once("exit", () => {
        clearTimeout(timer);
        resolve(true);
      });
    });
    if (!exited) child.kill("SIGKILL");
  };

  const fail = async (message: string): Promise<never> => {
    await stopChild();
    if (ownedDir) await rm(dataDir, { recursive: true, force: true }).catch(() => undefined);
    const tail = stderrTail.length > 0 ? `\nstderr tail:\n${stderrTail.join("\n")}` : "";
    throw new Error(`${message}${tail}`);
  };

  try {
    const readiness = await waitForReadiness(child, options.readyTimeoutMs ?? DEFAULT_READY_TIMEOUT_MS);
    try {
      assertProtocolCompatible(readiness.protocol_version);
    } catch (error) {
      await fail((error as Error).message);
    }
    let closed = false;
    return {
      readiness,
      dataDir,
      close: async () => {
        if (closed) return;
        closed = true;
        await stopChild();
        if (ownedDir) await rm(dataDir, { recursive: true, force: true }).catch(() => undefined);
      },
    };
  } catch (error) {
    await stopChild();
    if (ownedDir) await rm(dataDir, { recursive: true, force: true }).catch(() => undefined);
    throw error;
  }
}

function waitForReadiness(child: ChildProcess, timeoutMs: number): Promise<ReadinessInfo> {
  return new Promise<ReadinessInfo>((resolve, reject) => {
    if (!child.stdout) {
      reject(new Error("spawned runtime has no stdout pipe"));
      return;
    }
    const lines = createInterface({ input: child.stdout });
    const timer = setTimeout(() => {
      lines.close();
      reject(new Error(`timed out after ${timeoutMs}ms waiting for the runtime readiness line`));
    }, timeoutMs);
    const done = (fn: () => void): void => {
      clearTimeout(timer);
      lines.close();
      fn();
    };
    lines.on("line", (line: string) => {
      const readiness = parseReadinessLine(line);
      if (readiness) done(() => resolve(readiness));
    });
    child.once("error", (error: Error) => {
      done(() => reject(new Error(`failed to spawn the runtime: ${error.message}`)));
    });
    child.once("exit", (code: number | null) => {
      done(() => reject(new Error(`runtime exited before reporting readiness (code ${code})`)));
    });
  });
}
