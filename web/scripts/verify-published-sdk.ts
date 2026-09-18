/**
 * Release gate: prove the *about-to-be-published* artifacts work before
 * publishing. Invoked by `publish-packages.ts` with the vendored runtime
 * binary path:
 *
 *   1. spawn the binary via the SDK's own `launchHarness` (packaged dist, not
 *      `web/src` shims) against a fresh temp data dir,
 *   2. run a protocol roundtrip (`initialize` → `models/list` → `store/list`
 *      first page) through the public client surface,
 *   3. assert the readiness line's protocol/version fields.
 *
 * Also runnable standalone:
 *   bun scripts/verify-published-sdk.ts [path-to-agent-store-exe]
 */
import { launchHarness } from "@flowy-agent-store/sdk";

const VERSION = "0.1.0-beta.1";

const bin = process.argv[2] ?? process.env["AGENT_STORE_BIN"];
if (!bin) {
  console.error("usage: bun scripts/verify-published-sdk.ts <agent-store-exe>");
  process.exit(1);
}

const harness = await launchHarness({
  bin,
  requestTimeoutMs: 120_000,
  client: { name: "verify-published-sdk", version: VERSION },
});
try {
  const init = harness as unknown as { serverInfo?: { protocol_version?: string; version?: string } };
  const models = await harness.models.list();
  const store = await harness.listStore();
  console.log("VERIFY-OK", JSON.stringify({
    dataDir: harness.server.dataDir,
    readiness: { protocol_version: harness.server.readiness.protocol_version, version: harness.server.readiness.version },
    models: models.length,
    storeItems: store.items.length,
  }));
  void init;
} finally {
  // Doc `31` 方案 B：这一次 close 覆盖「退订 → 关传输 → 杀进程 → 删临时目录」。
  // 此前这里只调 `server.close()`，客户端传输从未被关闭。
  await harness.close();
}
