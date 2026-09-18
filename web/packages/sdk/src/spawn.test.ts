import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APP_SERVER_PROTOCOL_VERSION } from "@flowy-agent-store/protocol";
import { launchHarness } from "./index";

/**
 * Spawn end-to-end (gated): needs a real runtime binary because it boots a
 * backend with a temp data dir (cold DB init, tens of seconds).
 *
 *   AGENT_STORE_E2E_BIN=C:/workspace/allo/target/debug/agent-store.exe bun run test
 */
const BIN = process.env["AGENT_STORE_E2E_BIN"];

(BIN ? describe : describe.skip)("spawn end-to-end", () => {
  it("spawns, initializes and lists the store, then cleans up", async () => {
    const harness = await launchHarness({
      bin: BIN,
      client: { name: "node-e2e", version: "0.1.0" },
    });
    try {
      // The runtime binary and the SDK must agree on the contract fingerprint;
      // compare against the constant so the two can never drift apart silently.
      expect(harness.handshake.protocol_version).toBe(APP_SERVER_PROTOCOL_VERSION);
      // Doc `31` 方案 B：`launchHarness` 返回的对象**就是** client —— 没有 `.client` 一跳。
      expect(typeof harness.conversations.create).toBe("function");
      const store = await harness.listStore();
      expect(Array.isArray(store.items)).toBe(true);
      expect(existsSync(harness.server.dataDir)).toBe(true);
    } finally {
      await harness.close();
    }
    expect(existsSync(harness.server.dataDir)).toBe(false);
  }, 240_000);
});
