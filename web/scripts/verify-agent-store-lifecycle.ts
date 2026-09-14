/**
 * Agent Store lifecycle acceptance: the five verbs, end to end, through the
 * **published client surface** (`AppServerClient.store`).
 *
 * This is the seam the other two layers do not cover. `store.test.ts` drives the
 * `store` sub-client against a fake host (orchestration only); `importer_e2e.rs`
 * drives the real server through HTTP (server semantics only). Neither exercises
 * a real transport with the real sub-client, which is exactly where a
 * "server says X, client reads Y" mistake would live.
 *
 * What it asserts, per verb:
 *   获取  a directory market is discoverable through `store.list` / `search`
 *   安装  `store.install({ waitForReady: true })` reports `ok` and `ready`
 *   使用  the installed item's components are visible and usable-edged
 *   禁用  `store.setEnabled(false)` really moves the runtime state
 *   卸载  `store.uninstall()` releases everything and the catalogue agrees
 *
 * It deliberately does **not** try to assert on the server's filesystem: a
 * client cannot see the host's disk, so "the skill directory is gone" is covered
 * by the Rust e2e (`importer_install_registers_components_into_runtime`), which
 * runs in-process. Pretending otherwise here would be the same kind of
 * unfalsifiable claim this whole batch was about removing.
 *
 * Usage:
 *   bun scripts/verify-agent-store-lifecycle.ts --ws ws://127.0.0.1:8787/api/app-server/ws
 *
 * The target must be a real App Server. Start one with:
 *   cargo run -p nomifun-web -- --port 8787 --api-only --insecure-no-auth
 *
 * Exit code 0 = every assertion held; 1 = at least one did not.
 */

import { AppServerClient } from "../src/lib/client";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function valueOf(flag: string): string | undefined {
  const args = process.argv.slice(2);
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : undefined;
}

const wsUrl = valueOf("--ws") ?? "ws://127.0.0.1:8787/api/app-server/ws";
const ENTRY = "lifecycle-demo";

let failures = 0;
function ok(name: string): void {
  console.log(`  ✓ ${name}`);
}
function fail(name: string, detail: string): void {
  failures += 1;
  console.error(`  ✗ ${name}: ${detail}`);
}
function check(name: string, condition: boolean, detail = ""): void {
  if (condition) ok(name);
  else fail(name, detail || "condition was false");
}

/**
 * A one-entry directory market: the smallest shape that still exercises every
 * component kind the installer branches on (an agent and a connector).
 */
function buildMarket(root: string): void {
  const plugin = join(root, "plugins", ENTRY);
  mkdirSync(join(root, ".codebuddy-plugin"), { recursive: true });
  mkdirSync(join(plugin, ".codebuddy-plugin"), { recursive: true });
  mkdirSync(join(plugin, "agents"), { recursive: true });
  mkdirSync(join(plugin, "skills", "demo-notes"), { recursive: true });
  writeFileSync(
    join(root, ".codebuddy-plugin", "marketplace.json"),
    JSON.stringify({
      name: "lifecycle",
      version: "0.1.0",
      plugins: [{ name: ENTRY, source: `./plugins/${ENTRY}`, description: "Lifecycle demo" }],
    }),
  );
  writeFileSync(
    join(plugin, ".codebuddy-plugin", "plugin.json"),
    JSON.stringify({ name: ENTRY, version: "1.0.0", agents: ["./agents"] }),
  );
  writeFileSync(
    join(plugin, "agents", "lead.md"),
    "---\nname: lead\ndescription: Lead\n---\n\nLead body.\n",
  );
  writeFileSync(
    join(plugin, "skills", "demo-notes", "SKILL.md"),
    "---\nname: demo-notes\ndescription: Demo notes\n---\n\nBody.\n",
  );
  writeFileSync(
    join(plugin, "cli.json"),
    JSON.stringify({ name: "demo-cli", init: "npm install -g demo" }),
  );
}

async function main(): Promise<void> {
  console.log(`Agent Store lifecycle acceptance → ${wsUrl}`);

  const marketRoot = mkdtempSync(join(tmpdir(), "agent-store-lifecycle-"));
  buildMarket(marketRoot);

  const client = new AppServerClient({
    wsUrl,
    client: { name: "lifecycle-acceptance", version: "1.0.0" },
    // The market mirror + install is a long call by design.
    requestTimeoutMs: 120_000,
  });

  try {
    await client.connect();
    ok("initialize handshake");

    // ---- 获取 -------------------------------------------------------------
    const added = await client.addMarketplace({
      source_kind: "directory",
      source: marketRoot,
    });
    const marketplaceId = added.marketplace_id;
    check("market/add registers the source", Boolean(marketplaceId), JSON.stringify(added));

    const found = await client.store.search(ENTRY);
    check("store.search finds the entry", found.length === 1, `got ${found.length}`);
    const item = found[0];
    check("a fresh entry reports not-installed", item.installed === false, JSON.stringify(item));

    // ---- 安装 -------------------------------------------------------------
    const installed = await client.store.install(item, { timeoutMs: 60_000 });
    check("store.install reports ok", installed.ok, JSON.stringify(installed.components));
    check(
      "store.install waits until the components are usable",
      installed.ready === true,
      `readyIssue=${installed.readyIssue ?? "none"} component=${installed.readyComponentId ?? "none"}`,
    );
    check(
      "per-component outcomes come back",
      installed.components.length > 0,
      "the server forwarded no outcomes",
    );
    // The CLI connector is documented as not registered in V1; it must be
    // reported, not silently dropped.
    check(
      "a CLI connector is reported as unsupported rather than dropped",
      installed.components.some((component) => component.code === "cli_connector_unsupported"),
      JSON.stringify(installed.components.map((component) => component.code ?? component.action)),
    );

    const afterInstall = await client.store.installed();
    check(
      "the catalogue agrees the item is installed",
      afterInstall.some((entry) => entry.entry_name === ENTRY),
      JSON.stringify(afterInstall.map((entry) => entry.entry_name)),
    );

    // ---- 使用 -------------------------------------------------------------
    const status = await client.getInstallStatus(installed.snapshotId!);
    const active = status.components.filter((component) => component.state !== "not-installed");
    check("components are registered and usable-edged", active.length > 0, JSON.stringify(status));
    const agent = active.find((component) => component.kind === "agent");
    if (agent) {
      const detail = await client.agents.get(agent.id);
      check("the installed expert resolves to a Preset", Boolean(detail.preset_id), JSON.stringify(detail));
    } else {
      fail("the demo market produced an agent component", JSON.stringify(active));
    }

    // ---- 禁用 -------------------------------------------------------------
    const disabled = await client.store.setEnabled(item, false);
    check("store.setEnabled(false) reports ok", disabled.ok, JSON.stringify(disabled.components));
    const disabledIds = disabled.components.map((component) => component.component_id);
    check(
      "the install record now reads disabled",
      (await client.getInstallStatus(installed.snapshotId!)).components
        .filter((component) => disabledIds.includes(component.id))
        .every((component) => component.state === "disabled"),
      "a component did not move to disabled",
    );
    const marker = disabled.components.find((component) => component.code === "skill_disable_flag_only");
    check(
      "a skill's flag-only marker is surfaced, not swallowed",
      marker !== undefined,
      JSON.stringify(disabled.components.map((component) => component.code ?? component.action)),
    );

    const reEnabled = await client.store.setEnabled(item, true);
    check("store.setEnabled(true) reports ok", reEnabled.ok, JSON.stringify(reEnabled.components));

    // ---- 卸载 -------------------------------------------------------------
    const released = await client.store.uninstall(item);
    check("store.uninstall reports ok", released.ok, JSON.stringify(released.components));
    check(
      "every component is reported removed",
      released.components.length > 0 && released.components.every((component) => component.action === "removed"),
      JSON.stringify(released.components),
    );
    check(
      "the install record is cleared",
      (await client.getInstallStatus(installed.snapshotId!)).components.every(
        (component) => component.state === "not-installed",
      ),
      "some component is still installed",
    );
    check(
      "the catalogue agrees the item is released",
      (await client.store.search(ENTRY))[0]?.installed === false,
      "store.list still reports installed",
    );

    // Uninstall must be re-entrant: a retry is the natural reaction to a timeout.
    const again = await client.store.uninstall({
      ...item,
      snapshot_id: installed.snapshotId,
      installed: true,
    });
    check(
      "uninstall is re-entrant (a retry succeeds)",
      again.ok,
      JSON.stringify(again.components),
    );
  } finally {
    client.close();
    rmSync(marketRoot, { recursive: true, force: true });
  }

  if (failures > 0) {
    console.error(`\n${failures} assertion(s) failed`);
    process.exit(1);
  }
  console.log("\nall lifecycle assertions held");
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`lifecycle acceptance could not run: ${message}`);
  console.error(
    "\nThis script needs a live App Server with a matching protocol fingerprint.\n" +
      "Start one with: cargo run -p nomifun-web -- --port 8787 --api-only --insecure-no-auth",
  );
  process.exit(1);
});
