import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { launchClient } from "./index";

/**
 * Spawn end-to-end (gated): needs a real runtime binary because it boots a
 * backend with a temp data dir (cold DB init, tens of seconds).
 *
 *   AGENT_STORE_E2E_BIN=C:/workspace/allo/target/debug/agent-store.exe bun run test
 */
const BIN = process.env["AGENT_STORE_E2E_BIN"];

(BIN ? describe : describe.skip)("spawn end-to-end", () => {
  it("spawns, initializes and lists the store, then cleans up", async () => {
    const session = await launchClient({
      bin: BIN,
      client: { name: "node-e2e", version: "0.1.0" },
    });
    try {
      expect(session.initializeResult.protocol_version).toBe("2026-08-26");
      const store = await session.client.listStore();
      expect(Array.isArray(store.items)).toBe(true);
      expect(existsSync(session.server.dataDir)).toBe(true);
    } finally {
      await session.close();
    }
    expect(existsSync(session.server.dataDir)).toBe(false);
  }, 240_000);
});
