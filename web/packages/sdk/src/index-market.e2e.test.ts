import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APP_SERVER_PROTOCOL_VERSION } from "@flowy-agent-store/protocol";
import { launchHarness } from "./index";

/**
 * Index-market end-to-end via the SDK (gated).
 *
 * It spawns the real `agent-store` runtime, adds the **published** experts
 * `index.json`, lists the catalogue, installs one expert archive (downloaded
 * from ModelScope on demand) and checks the card/snapshot facts that only a real
 * host can produce.
 *
 *   cargo build -p agent-store
 *   AGENT_STORE_E2E_BIN=C:/workspace/allo/target/debug/agent-store.exe \
 *     bun run test -- index-market
 *
 * Skipped by default: it needs network + a freshly built binary (the vendored
 * one predates `fp-20`).
 */
const BIN = process.env["AGENT_STORE_E2E_BIN"];
const INDEX_URL =
  process.env["AGENT_STORE_E2E_INDEX_URL"] ??
  "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/wb-market/experts/index.json";
const ENTRY = process.env["AGENT_STORE_E2E_ENTRY"] ?? "accessibility-auditor";
const SKILLS_URL =
  process.env["AGENT_STORE_E2E_SKILLS_URL"] ??
  "https://www.modelscope.cn/models/me9rez/flowy-marketplace/resolve/master/wb-market/skills/index.json";

(BIN ? describe : describe.skip)("index market end-to-end", () => {
  it("adds the published index, lists it and installs one entry on demand", async () => {
    const harness = await launchHarness({
      bin: BIN as string,
      client: { name: "sdk-index-e2e", version: "0.1.0" },
      // Register nothing at boot: this test adds exactly the market it audits.
      marketDownload: "none",
      requestTimeoutMs: 180_000,
    });
    try {
      expect(harness.handshake.protocol_version).toBe(APP_SERVER_PROTOCOL_VERSION);

      const market = await harness.addMarketplace({
        name: "experts-e2e",
        source_kind: "index",
        source: INDEX_URL,
      });
      expect(market.source_kind).toBe("index");
      // The published market currently has 421 entries; assert the order of
      // magnitude so a content update does not turn this into a tripwire.
      expect(market.entry_count).toBeGreaterThan(400);
      expect(market.resolved_revision).toMatch(/^[0-9a-f]{64}$/);

      const items = await harness.store.list();
      const experts = items.filter((item) => item.marketplace_id === market.marketplace_id);
      expect(experts.length).toBe(market.entry_count);

      // Card fields that only an index row can supply (no entry dir on disk).
      const withPrompts = experts.find((item) => item.entry_name === "a-share-analysis");
      if (withPrompts) {
        expect(withPrompts.default_init_prompt?.zh ?? "").not.toBe("");
        expect((withPrompts.quick_prompts ?? []).length).toBeGreaterThan(0);
      }
      const target = experts.find((item) => item.entry_name === ENTRY) ?? experts[0];
      expect(target.icon_url).toMatch(/^https:\/\//);
      expect(target.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(target.download_size ?? 0).toBeGreaterThan(0);
      expect(target.available).toBe(true);
      // The SDK resolves the absolute icon without needing the host base URL.
      expect(harness.store.avatarUrl(target)).toBe(target.icon_url);

      // The archive is fetched at install time; readiness (runtime probes) is
      // orthogonal to this contract, so it is not awaited here.
      const installed = await harness.store.install(target, { waitForReady: false });
      expect(installed.snapshotId).not.toBeNull();
      expect(installed.ok, installed.errors.join("; ")).toBe(true);
      expect(existsSync(harness.server.dataDir)).toBe(true);

      const after = await harness.store.list();
      const refreshed = after.find(
        (item) =>
          item.marketplace_id === market.marketplace_id && item.entry_name === target.entry_name,
      );
      expect(refreshed?.installed).toBe(true);
    } finally {
      await harness.close();
    }
  }, 300_000);

  it("installs a skill from the shared payload index", async () => {
    const harness = await launchHarness({
      bin: BIN as string,
      client: { name: "sdk-index-payload-e2e", version: "0.1.0" },
      marketDownload: "none",
      requestTimeoutMs: 300_000,
    });
    try {
      const market = await harness.addMarketplace({
        name: "skills-e2e",
        source_kind: "index",
        source: SKILLS_URL,
      });
      expect(market.source_kind).toBe("index");
      expect(market.entry_count).toBeGreaterThan(1500);

      const items = await harness.store.list();
      const skills = items.filter((item) => item.marketplace_id === market.marketplace_id);
      const target = skills[0];
      expect(target.available).toBe(true);
      expect(target.download_size ?? 0).toBeGreaterThan(0);
      // Payload markets carry no per-entry digest: the shared payload zip is
      // the integrity anchor, and `member` names the entry inside it.
      expect(target.sha256 ?? null).toBeNull();

      const installed = await harness.store.install(target, { waitForReady: false });
      expect(installed.snapshotId).not.toBeNull();
      expect(installed.ok, installed.errors.join("; ")).toBe(true);

      const after = await harness.store.list();
      const refreshed = after.find(
        (item) =>
          item.marketplace_id === market.marketplace_id && item.entry_name === target.entry_name,
      );
      expect(refreshed?.installed).toBe(true);
    } finally {
      await harness.close();
    }
  }, 300_000);
});
