import { spawn, type ChildProcess } from "node:child_process";
import { describe, expect, it } from "vitest";
import { waitForReadiness } from "./spawn";

/**
 * Regression for the stdout backpressure defect: once the readiness line is
 * parsed the SDK must keep draining `child.stdout`. `readline.close()` pauses
 * the input stream, so a chatty child fills the OS pipe buffer and then blocks
 * on `drain` forever — long sessions (multi-turn runs, market scans) hang with
 * no error. This test spawns a synthetic runtime that only reaches
 * `process.exit(0)` when someone keeps reading stdout.
 */
const READY_TIMEOUT_MS = 10_000;
const EXIT_TIMEOUT_MS = 5_000;

const SYNTHETIC_RUNTIME = [
  'const ready = { agent_store: "listening", host: "127.0.0.1", port: 45678,',
  '  url: "http://127.0.0.1:45678", protocol_version: "test-version",',
  '  version: "0.0.0-test", auth: "none" };',
  'process.stdout.write(JSON.stringify(ready) + "\\n");',
  "const CHUNK = Buffer.alloc(64 * 1024, 0x61);",
  "const TOTAL = 4 * 1024 * 1024;",
  "let remaining = TOTAL;",
  "function pump() {",
  "  while (remaining > 0) {",
  "    remaining -= CHUNK.length;",
  '    if (!process.stdout.write(CHUNK)) { process.stdout.once("drain", pump); return; }',
  "  }",
  "  process.exit(0);",
  "}",
  "pump();",
].join("\n");

function waitForExit(child: ChildProcess, timeoutMs: number): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolve(true);
      return;
    }
    const timer = setTimeout(() => resolve(false), timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

describe("waitForReadiness stdout backpressure", () => {
  it("keeps draining stdout so a chatty child can exit after readiness", async () => {
    const child = spawn(process.execPath, ["-e", SYNTHETIC_RUNTIME], {
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    try {
      const readiness = await waitForReadiness(child, READY_TIMEOUT_MS);
      expect(readiness.port).toBe(45678);

      const exited = await waitForExit(child, EXIT_TIMEOUT_MS);
      expect(exited).toBe(true);
    } finally {
      if (child.exitCode === null && child.signalCode === null) child.kill();
    }
  }, 30_000);
});
