/**
 * T15 live acceptance: W13 marketplace management + AC-5 artifact data path.
 *
 * Runs against a real App Server (`target/release/agent-store.exe --port 8787`)
 * through the **Web host client** (`web/src/lib/client.ts`) — the exact surface
 * `CatalogView` and `ArtifactPanel` call. Uses a **local directory market**
 * fixture, so nothing here depends on the public mirror.
 *
 * Usage:
 *   bun scripts/sdk-live-w13-acceptance.ts [ws://127.0.0.1:8787/api/app-server/ws]
 */
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { AppServerClient } from "../src/lib/client";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
/**
 * A local directory market for the registry test. `software-company` carries
 * `.codebuddy-plugin/plugin.json`, so the probe resolves it as a plugin root
 * (one entry) with no network involved. (`skill-market` is a skill-root layout
 * whose manifest has no `skills` array — `market/add` correctly rejects it.)
 */
const LOCAL_MARKET = join(REPO_ROOT, "crates/backend/nomifun-importer/tests/fixtures/software-company");
const WS_URL = process.argv[2] ?? "ws://127.0.0.1:8787/api/app-server/ws";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 300)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}

const client = new AppServerClient({
  wsUrl: WS_URL,
  client: { name: "t15-acceptance", version: "0.1.0" },
  // Generous: the first `store/list` mirrors the default markets (see D-SDK-1).
  requestTimeoutMs: 180_000,
});
await client.connect();

// ── AC-5 · the artifact panel's data path (host file service) ────────────────
const workspace = await mkdtemp(join(tmpdir(), "t15-artifacts-"));
await writeFile(join(workspace, "report.md"), "# Report\n\nhello **artifacts**\n");
await writeFile(join(workspace, "notes.txt"), "plain text artifact\n");

const files = await client.listWorkspaceFiles(workspace);
check("AC-5 fs/list returns both artifacts", files.length === 2, files.map((file) => file.relative_path));

const markdown = files.find((file) => file.name === "report.md");
const content = markdown ? await client.readFileContent(markdown.full_path, workspace) : null;
check(
  "AC-5 fs/read returns the markdown body",
  typeof content === "string" && content.includes("artifacts"),
  content?.slice(0, 40),
);

const metadata = markdown ? await client.getFileMetadata(markdown.full_path, workspace) : null;
check(
  "AC-5 fs/metadata returns size + mime",
  metadata !== null && metadata.size > 0 && metadata.type.includes("markdown"),
  { size: metadata?.size, type: metadata?.type },
);

// ── W13 · market registry, auto-update round-trip, entry import, cascade remove
const added = await client.addMarketplace({ source_kind: "directory", source: LOCAL_MARKET });
check("W13 market/add registers a local market", added.entry_count > 0, {
  id: added.marketplace_id,
  entries: added.entry_count,
  auto_update: added.auto_update,
  enabled: added.enabled,
  version: added.version,
});
check("W13 a third-party source does not auto-update (Q7 ①)", added.auto_update === false, added.auto_update);

const toggled = await client.setMarketplaceAutoUpdate(added.marketplace_id, true);
const listed = (await client.listMarketplaces()).find((market) => market.marketplace_id === added.marketplace_id);
check(
  "W13 auto-update round-trips through market/list",
  toggled.auto_update === true && listed?.auto_update === true,
  { response: toggled.auto_update, read_back: listed?.auto_update },
);

const detail = await client.getMarketplace(added.marketplace_id);
const entry = detail.entries[0];
check("W13 market/get exposes entries", entry !== undefined, entry?.name);

if (entry) {
  const imported = await client.importMarketplaceEntry(added.marketplace_id, entry.name);
  check(
    "W13 market/entry-import produces a provenance snapshot",
    Boolean(imported.snapshot_id && imported.content_digest),
    { snapshot_id: imported.snapshot_id, reused: imported.reused, digest: imported.content_digest?.slice(0, 16) },
  );
  const imports = await client.listImports();
  check(
    "W13 import/list shows the new snapshot",
    imports.some((item) => item.snapshot_id === imported.snapshot_id),
  );
}

const removed = await client.removeMarketplace(added.marketplace_id, true);
check("W13 market/remove returns the cascade impact", Array.isArray(removed.snapshots), removed);
const afterRemove = await client.listMarketplaces();
check(
  "W13 market/list no longer contains the removed market",
  !afterRemove.some((market) => market.marketplace_id === added.marketplace_id),
);

client.close();
console.log(failures === 0 ? "T15-ALL-PASS" : `T15-FAILURES=${failures}`);
process.exit(failures === 0 ? 0 : 1);
