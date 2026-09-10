import type { ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { spawnRuntimeChild, type SpawnExitInfo } from "./spawn";

/**
 * Runtime lifecycle (A1 / T2, `16` §5.2): `env` / `cwd` passthrough and an
 * `exited` signal that also fires on an abnormal end — otherwise a crashed
 * runtime is indistinguishable from one that is still starting up.
 *
 * A synthetic child is used because the managed CLI args `spawnAppServer`
 * adds cannot be fed to an arbitrary executable.
 */

const PROBE = [
  'const fs = require("node:fs");',
  'fs.writeFileSync("marker.txt", "ok");',
  'process.stdout.write(JSON.stringify({',
  '  probe: process.env.AGENT_STORE_TEST_ENV ?? null,',
  '  inherited: typeof process.env.PATH === "string" && process.env.PATH.length > 0,',
  '}) + "\\n");',
].join("\n");

function collectOutput(child: ChildProcess): Promise<string> {
  return new Promise<string>((resolve) => {
    let out = "";
    child.stdout?.on("data", (chunk: Buffer) => {
      out += String(chunk);
    });
    // `close` (not `exit`): stdio is flushed by then.
    child.once("close", () => resolve(out));
  });
}

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`not settled within ${ms}ms`)), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

describe("spawnRuntimeChild", () => {
  it("merges env over the parent env and honours cwd", async () => {
    const dir = await mkdtemp(join(tmpdir(), "agent-store-sdk-lifecycle-"));
    try {
      const { child, exited } = spawnRuntimeChild(process.execPath, ["-e", PROBE], {
        env: { AGENT_STORE_TEST_ENV: "from-option" },
        cwd: dir,
      });

      const output = collectOutput(child);
      const info = await withTimeout(exited, 5_000);

      expect(info).toEqual({ code: 0, signal: null });
      expect(JSON.parse((await output).trim())).toEqual({
        probe: "from-option",
        inherited: true,
      });
      expect(existsSync(join(dir, "marker.txt"))).toBe(true);
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  });

  it("settles exited and calls onExit when the child is killed", async () => {
    const seen: SpawnExitInfo[] = [];
    const { child, exited } = spawnRuntimeChild(
      process.execPath,
      ["-e", "setInterval(() => {}, 1000);"],
      { onExit: (info) => seen.push(info) },
    );

    child.kill();

    const info = await withTimeout(exited, 5_000);
    expect(info.code !== null || info.signal !== null).toBe(true);
    expect(seen).toEqual([info]);
  });

  it("reports a non-zero exit code instead of hanging", async () => {
    const { exited } = spawnRuntimeChild(process.execPath, ["-e", "process.exit(3);"]);

    const info = await withTimeout(exited, 5_000);
    expect(info.code).toBe(3);
    expect(info.signal).toBe(null);
  });
});
