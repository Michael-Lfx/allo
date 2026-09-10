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
import { APP_SERVER_PROTOCOL_VERSION } from "@flowy-agent-store/protocol";
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
  /** Extra environment variables, merged over the parent's `process.env`. */
  env?: Record<string, string | undefined>;
  /** Working directory for the child. Defaults to the parent's cwd. */
  cwd?: string;
  /** Called once when the child exits, for any reason (crash included). */
  onExit?: (info: SpawnExitInfo) => void;
}

/** How a spawned runtime process ended. */
export interface SpawnExitInfo {
  /** Exit code, or `null` when the process was terminated by a signal. */
  code: number | null;
  /** Terminating signal, or `null` on a normal exit. */
  signal: string | null;
}

export interface SpawnedServer {
  readonly readiness: ReadinessInfo;
  readonly dataDir: string;
  /**
   * Settles when the runtime process exits — including an unexpected crash,
   * which is otherwise indistinguishable from "still starting up". Never
   * rejects: inspect `code` / `signal`.
   */
  readonly exited: Promise<SpawnExitInfo>;
  /** Terminate the child and remove the data dir when it was auto-created. */
  close(): Promise<void>;
}

export async function spawnAppServer(options: SpawnOptions = {}): Promise<SpawnedServer> {
  const bin = resolveAppServerBin(options.bin);
  const ownedDir = options.dataDir === undefined;
  const dataDir = ownedDir ? await mkdtemp(join(tmpdir(), "agent-store-sdk-")) : options.dataDir as string;
  const port = options.port ?? 0;

  const { child, exited } = spawnRuntimeChild(
    bin,
    ["--host", "127.0.0.1", "--port", String(port), "--data-dir", dataDir, "--no-open", ...(options.extraArgs ?? [])],
    options,
  );

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
      exited,
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

/**
 * Spawn the runtime process and start tracking its exit.
 *
 * Internal (not part of the package entry): exported so the lifecycle and
 * `env` / `cwd` passthrough contract can be exercised with a synthetic child,
 * which is what the managed CLI args normally make impossible to inject.
 */
export function spawnRuntimeChild(
  bin: string,
  args: string[],
  options: Pick<SpawnOptions, "env" | "cwd" | "onExit"> = {},
): { child: ChildProcess; exited: Promise<SpawnExitInfo> } {
  const child = spawn(bin, args, {
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    ...(options.env ? { env: { ...process.env, ...options.env } } : {}),
    ...(options.cwd ? { cwd: options.cwd } : {}),
  });
  const exited = new Promise<SpawnExitInfo>((resolve) => {
    child.once("exit", (code, signal) => {
      const info: SpawnExitInfo = { code, signal };
      resolve(info);
      try {
        options.onExit?.(info);
      } catch {
        // An exit observer must never destabilize the exit path.
      }
    });
  });
  return { child, exited };
}

/** Internal: exported for the stdout-backpressure regression test only. */
export function waitForReadiness(child: ChildProcess, timeoutMs: number): Promise<ReadinessInfo> {
  return new Promise<ReadinessInfo>((resolve, reject) => {
    if (!child.stdout) {
      reject(new Error("spawned runtime has no stdout pipe"));
      return;
    }
    const stdout = child.stdout;
    const lines = createInterface({ input: stdout });
    let settled = false;
    const fail = (error: Error): void => {
      if (settled) return;
      settled = true;
      lines.close();
      reject(error);
    };
    const timer = setTimeout(() => {
      fail(new Error(`timed out after ${timeoutMs}ms waiting for the runtime readiness line`));
    }, timeoutMs);
    lines.on("line", (line: string) => {
      if (settled) return;
      const readiness = parseReadinessLine(line);
      if (!readiness) return;
      settled = true;
      clearTimeout(timer);
      lines.close();
      // `readline.close()` pauses the underlying stream. Without an active
      // reader the child blocks once the OS pipe buffer (~64KB) fills, so keep
      // draining stdout for the child's whole lifetime (output is discarded).
      stdout.resume();
      resolve(readiness);
    });
    child.once("error", (error: Error) => {
      fail(new Error(`failed to spawn the runtime: ${error.message}`));
    });
    child.once("exit", (code: number | null) => {
      fail(new Error(`runtime exited before reporting readiness (code ${code})`));
    });
  });
}
