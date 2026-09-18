/**
 * Release gate: prove the *about-to-be-published* artifacts work before
 * publishing. Invoked by `publish-packages.ts` with the vendored runtime
 * binary path:
 *
 *   1. spawn the binary via the SDK's own `launchClient` (packaged dist, not
 *      `web/src` shims) against a fresh temp data dir,
 *   2. run a protocol roundtrip (`initialize` → `models/list` → `store/list`
 *      first page) through the public client surface,
 *   3. assert the readiness line's protocol/version fields.
 *
 * Also runnable standalone:
 *   bun scripts/verify-published-sdk.ts [path-to-agent-store-exe]
 */
import { launchClient } from "@flowy-agent-store/sdk";

const VERSION = "0.1.0-beta.1";

const bin = process.argv[2] ?? process.env["AGENT_STORE_BIN"];
if (!bin) {
  console.error("usage: bun scripts/verify-published-sdk.ts <agent-store-exe>");
  process.exit(1);
}

const { server, client } = await launchClient({
  bin,
  requestTimeoutMs: 120_000,
  client: { name: "verify-published-sdk", version: VERSION },
});
try {
  const init = client as unknown as { serverInfo?: { protocol_version?: string; version?: string } };
  const models = await client.models.list();
  const store = await client.listStore();
  console.log("VERIFY-OK", JSON.stringify({
    dataDir: server.dataDir,
    readiness: { protocol_version: server.readiness.protocol_version, version: server.readiness.version },
    models: models.length,
    storeItems: store.items.length,
  }));
  void init;
} finally {
  await server.close();
}
