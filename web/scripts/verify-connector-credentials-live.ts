/**
 * Live acceptance for connector user credentials (doc `34` §9.1).
 *
 * This is the one thing the Rust layers cannot prove about themselves. The
 * importer tests stop at the stored template, the `nomifun-mcp` unit tests stop at
 * the resolved header map, and the app-server e2e stops at `[credentials]` — none
 * of them ever *sends* a request. So the two claims that only a real socket can
 * settle are settled here, against a mock MCP server that records what it
 * receives:
 *
 *   1. **fail-closed** — with the credential unset, `connector/test` reports the
 *      missing key and the mock receives **nothing**. A half-resolved
 *      `Authorization: Bearer ${secret:KEY}` must never leave the process;
 *   2. **the resolved truth** — after `credential/set`, the header the mock
 *      receives is the template with the value substituted, prefix and all
 *      (`Bearer <key>`), never the literal `${secret:KEY}`.
 *
 * The two shapes `34` §9.1 names that fit a streamable-HTTP mock are covered: a
 * mixed form (3 plain + 1 secret, with the url *and* the header built from
 * templates) and a two-secret form (`CLIENT_ID` + `CLIENT_SECRET`). The `sse`
 * shape and the empty-value shape are covered by the Rust layers (`sse` survives
 * import verbatim; an empty `env` value becomes a reference), because they need a
 * mock with a different transport shape rather than a different assertion.
 *
 * A third connector is **not** installed from anywhere: it is handed to
 * `connector/register` (`34` §6.5), the external developer's route in — its own
 * template is its credential declaration, and the same two calls (`setCredentials`
 * then `test`) carry its key to the wire. That entry stays on the host afterwards:
 * there is no unregister method yet (registered in `34` §10), which is why this
 * script expects a disposable host.
 *
 * `clear` is checked too: it must put the connector back to `requires_input`
 * *and* stop the traffic again.
 *
 * Usage:
 *   bun scripts/verify-connector-credentials-live.ts --ws ws://127.0.0.1:8903/api/app-server/ws
 *
 * The target must be a real Agent Store host in local mode, started **fresh**.
 * Start one with:
 *   target/debug/agent-store.exe --port 8903 --no-open --data-dir <scratch>
 *
 * `market/add` for a **directory** source writes a marketplace row that is checked
 * against `marketplace_id = lower(marketplace_id)`, so the fixture's directory name
 * is built lowercase — an uppercase letter there fails the insert with a CHECK
 * violation that reads like a server bug.
 *
 * A **second** run against the same host stops at `store/install`, which reports
 * `ok: false` with zero components for content it has installed and uninstalled
 * once already. That is a store re-install behaviour with no credential in it (the
 * install fails before any credential operation), reproduced by running this
 * script twice; `34` §10 registers it. The run sweeps its own leftover market
 * first, so a crashed run does not poison the next one.
 *
 * Exit code 0 = every assertion held; 1 = at least one did not.
 */

import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { AppServerClient } from "../src/lib/client";

function valueOf(flag: string): string | undefined {
  const args = process.argv.slice(2);
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : undefined;
}

const wsUrl = valueOf("--ws") ?? "ws://127.0.0.1:8903/api/app-server/ws";

const MIXED_KEY = "live-mixed-key-value";
const PAIR_ID = "live-client-id-value";
const PAIR_SECRET = "live-client-secret-value";
const DEV_KEY = "the-developers-own-key";

/** The market's own name, as `connectors.json` declares it. */
const MARKET_NAME = "credential-live";

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

/** What the mock MCP server saw, in order. */
interface Seen {
  method: string;
  authorization: string | null;
  clientId: string | null;
}

/**
 * A temp directory whose **name is all lowercase**.
 *
 * `mkdtempSync` appends base62, and `market/add` derives a directory market's id
 * from the path (`sanitize_slug` keeps case) — but the marketplace row is checked
 * against `marketplace_id = lower(marketplace_id)`, so a stray uppercase letter
 * fails the insert with a CHECK violation that reads like a server bug. The id has
 * to be quoted as lowercase, so the directory name is built that way.
 */
function mkdtempLowercase(prefix: string): string {
  const path = join(tmpdir(), `${prefix}${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(path, { recursive: true });
  return path;
}

/**
 * A streamable-HTTP MCP server that answers `initialize` + `tools/list` and
 * records every request's method and credential-bearing headers.
 *
 * It also **refuses** anything without the bearer token, so a probe that somehow
 * skipped resolution would be caught by the mock rather than pass by accident.
 */
function startMockMcp() {
  const seen: Seen[] = [];
  const server = Bun.serve({
    port: 0,
    fetch: async (request) => {
      const body = (await request.json().catch(() => ({}))) as {
        id?: unknown;
        method?: string;
      };
      const method = body.method ?? "";
      const authorization = request.headers.get("authorization");
      const clientId = request.headers.get("x-client-id");
      seen.push({ method, authorization, clientId });

      if (method === "initialize") {
        return Response.json({
          jsonrpc: "2.0",
          id: body.id ?? null,
          result: {
            protocolVersion: "2025-11-25",
            capabilities: {},
            serverInfo: { name: "mock-key-mcp", version: "1" },
          },
        });
      }
      if (method === "notifications/initialized") {
        return new Response(null, { status: 202 });
      }
      if (method === "tools/list") {
        if (!authorization?.startsWith("Bearer ")) {
          return new Response("unauthorized", { status: 401 });
        }
        return Response.json({
          jsonrpc: "2.0",
          id: body.id ?? null,
          result: {
            tools: [
              {
                name: "echo",
                description: "Echo the argument back",
                inputSchema: { type: "object", properties: { text: { type: "string" } } },
              },
            ],
          },
        });
      }
      return Response.json({
        jsonrpc: "2.0",
        id: body.id ?? null,
        error: { code: -32601, message: `unexpected method ${method}` },
      });
    },
  });
  return { server, seen, port: server.port };
}

/**
 * A one-market, two-connector directory market in CodeBuddy's real layout:
 *   <root>/.codebuddy-connector/connectors.json   (the index the importer reads back)
 *   <root>/connectors/<id>/{mcp.json,token-schema.json,skills/<dir>/SKILL.md}
 */
function buildMarket(root: string, mockUrl: string): void {
  mkdirSync(join(root, ".codebuddy-connector"), { recursive: true });
  writeFileSync(
    join(root, ".codebuddy-connector", "connectors.json"),
    JSON.stringify({
      name: "credential-live",
      version: "0.1.0",
      connectors: [
        { id: "demo-mixed", name: "demo-mixed", version: "1.0.0", type: "mcp", auth_mode: "token" },
        { id: "demo-pair", name: "demo-pair", version: "1.0.0", type: "mcp", auth_mode: "token" },
      ],
    }),
  );

  const entries: [string, unknown, unknown][] = [
    [
      // The **normalized** spelling (`${secret:NAME}`), which is also this
      // importer's own output: a hand-authored directory may well be written that
      // way, and a rewrite that prefixed it a second time produced
      // `${secret:secret:NAME}` — a reference nothing can resolve.
      "demo-mixed",
      {
        mcpServers: {
          "demo-mixed": {
            type: "streamableHttp",
            url: `${mockUrl}/mcp`,
            headers: { Authorization: "Bearer ${secret:DEMO_API_KEY}" },
          },
        },
      },
      {
        title: "混合表单示例",
        fields: [
          { key: "DEMO_SCHEMA", type: "text", label: "协议", defaultValue: "http" },
          { key: "DEMO_HOST", type: "text", label: "主机", defaultValue: "127.0.0.1" },
          { key: "DEMO_PORT", type: "text", label: "端口", defaultValue: "1" },
          { key: "DEMO_API_KEY", type: "password", label: "密钥", required: true },
        ],
      },
    ],
    [
      // The **bare** spelling, which is what the real market writes (`34` §5.4):
      // the importer turns it into the normalized form, so both ends of that
      // transformation are exercised by one run.
      "demo-pair",
      {
        mcpServers: {
          "demo-pair": {
            type: "streamableHttp",
            url: `${mockUrl}/mcp`,
            headers: {
              "X-Client-Id": "${CLIENT_ID}",
              Authorization: "Bearer ${CLIENT_SECRET}",
            },
          },
        },
      },
      {
        title: "双字段示例",
        fields: [
          { key: "CLIENT_ID", type: "password", label: "客户端 ID", required: true },
          { key: "CLIENT_SECRET", type: "password", label: "客户端密钥", required: true },
        ],
      },
    ],
  ];

  for (const [id, mcp, schema] of entries) {
    mkdirSync(join(root, "connectors", id, "skills", "demo"), { recursive: true });
    writeFileSync(join(root, "connectors", id, "mcp.json"), JSON.stringify(mcp));
    writeFileSync(join(root, "connectors", id, "token-schema.json"), JSON.stringify(schema));
    writeFileSync(
      join(root, "connectors", id, "skills", "demo", "SKILL.md"),
      "---\nname: demo\ndescription: A demo skill the fixture needs so the import is clean.\n---\n\nbody\n",
    );
  }
}

async function main(): Promise<void> {
  console.log(`Connector credential live acceptance → ${wsUrl}`);

  const marketRoot = mkdtempLowercase("as-credential-live-");
  const mock = startMockMcp();
  const mockUrl = `http://127.0.0.1:${mock.port}`;
  buildMarket(marketRoot, mockUrl);

  const client = new AppServerClient({
    wsUrl,
    client: { name: "connector-credential-live", version: "1.0.0" },
    requestTimeoutMs: 60_000,
  });

  try {
    await client.connect();
    ok("initialize handshake");

    // A run that died before its cleanup leaves its market registered, and an entry
    // of the same name in it shadows this run's (`store/install` resolves by entry
    // name, and the stale one points at a deleted directory). Sweeping first is what
    // makes this script safe to run twice against one host.
    for (const market of await client.listMarketplaces()) {
      if (market.name === MARKET_NAME) {
        await client.removeMarketplace(market.marketplace_id, true);
        console.log(`  · swept a leftover "${MARKET_NAME}" market from an earlier run`);
      }
    }

    const added = await client.addMarketplace({ source_kind: "directory", source: marketRoot });
    check("market/add registers the source", Boolean(added.marketplace_id), JSON.stringify(added));

    for (const entry of ["demo-mixed", "demo-pair"]) {
      // Scoped to the market **this run** registered: `store/search` is global, so
      // a second run against the same host would otherwise pick the previous run's
      // twin — whose directory has been deleted — and install something that is not
      // there.
      const found = (await client.store.search(entry)).filter(
        (item) => item.marketplace_id === added.marketplace_id,
      );
      check(`store/search finds ${entry}`, found.length === 1, `got ${found.length}`);
      const installed = await client.store.install(found[0], { timeoutMs: 60_000 });
      check(`store/install registers ${entry}`, installed.ok, JSON.stringify(installed));
    }

    // ---- 0. the external developer's own server (34 §6.5) ------------------
    // No marketplace, no `token-schema.json`, nothing installed: the template the
    // caller hands over **is** the declaration, and the form has to appear from it.
    // Registered before the catalog is read, so the list below is the one every
    // other assertion works from.
    const registered = await client.connectors.register({
      name: "dev-owned-mcp",
      description: "The developer's own server",
      transport: {
        type: "http",
        url: `${mockUrl}/mcp`,
        headers: { Authorization: "Bearer ${secret:DEV_KEY}" },
      },
    });
    check(
      "connector/register accepts a hand-made server",
      registered.name === "dev-owned-mcp" && Boolean(registered.id),
      JSON.stringify(registered).slice(0, 300),
    );
    check(
      "…and its own template became the credential form",
      registered.credential?.mode === "token" &&
        registered.credential.fields.length === 1 &&
        registered.credential.fields[0].key === "DEV_KEY" &&
        registered.credential.fields[0].kind === "secret" &&
        registered.credential.fields[0].required,
      JSON.stringify(registered.credential),
    );
    // Registration grants no connection: the row comes up disabled, and the probe
    // below is what decides whether it may be enabled.
    check(
      "…and it is not enabled by registering it",
      registered.enabled === false,
      JSON.stringify({ enabled: registered.enabled }),
    );

    const listed = await client.connectors.list();
    const byName = (name: string) => {
      const found = listed.find((connector) => connector.name === name);
      if (!found) throw new Error(`${name} is not in the connector catalog`);
      return found;
    };

    // ---- 1. nothing is sent while the credential is missing ---------------
    for (const [name, key, missing] of [
      ["demo-mixed", MIXED_KEY, "DEMO_API_KEY"],
      ["demo-pair", PAIR_ID, "CLIENT_ID"],
      ["dev-owned-mcp", DEV_KEY, "DEV_KEY"],
    ] as const) {
      const connector = byName(name);
      check(
        `${name} reports the credential as missing, not as oauth`,
        connector.credential?.mode === "token" && connector.credential.status === "requires_input",
        JSON.stringify(connector.credential),
      );
      check(
        `${name} names the missing key`,
        connector.credential?.missing.includes(missing) === true,
        JSON.stringify(connector.credential?.missing),
      );

      const probe = await client.connectors.test(connector.id);
      check(
        `${name}: probe refuses with a typed missing-credential code`,
        !probe.success && probe.code === "MCP_MISSING_CREDENTIAL",
        JSON.stringify(probe),
      );
      // Deliberately not echoing the value here: the error names keys only.
      check(
        `${name}: the error names the key and carries no value`,
        (probe.error ?? "").includes(missing) && !(probe.error ?? "").includes(key),
        JSON.stringify(probe.error),
      );
      check(
        `${name}: **the mock received nothing** — fail-closed, not a failed request`,
        mock.seen.length === 0,
        JSON.stringify(mock.seen),
      );
    }

    // ---- 2. after set, the resolved truth goes out ------------------------
    const mixed = byName("demo-mixed");
    const mixedSet = await client.connectors.setCredentials(mixed.id, { DEMO_API_KEY: MIXED_KEY });
    check(
      "demo-mixed: set leaves it configured and missing nothing",
      mixedSet.status === "configured" && mixedSet.missing.length === 0,
      JSON.stringify(mixedSet),
    );
    check(
      "demo-mixed: set never echoes the value it stored",
      !JSON.stringify(mixedSet).includes(MIXED_KEY),
      JSON.stringify(mixedSet),
    );

    mock.seen.length = 0;
    const mixedProbe = await client.connectors.test(mixed.id);
    check("demo-mixed: probe succeeds", mixedProbe.success, JSON.stringify(mixedProbe));
    check(
      "demo-mixed: the tool list comes back",
      (mixedProbe.tools ?? []).some((tool) => tool.name === "echo"),
      JSON.stringify(mixedProbe.tools),
    );
    const mixedSeen = mock.seen.filter((entry) => entry.method === "tools/list");
    check(
      "demo-mixed: the header that arrived is the template resolved",
      mixedSeen.length > 0 && mixedSeen.every((entry) => entry.authorization === `Bearer ${MIXED_KEY}`),
      JSON.stringify(mock.seen),
    );
    check(
      "demo-mixed: no request ever carried the literal reference",
      !JSON.stringify(mock.seen).includes("${secret:"),
      JSON.stringify(mock.seen),
    );

    const pair = byName("demo-pair");
    await client.connectors.setCredentials(pair.id, {
      CLIENT_ID: PAIR_ID,
      CLIENT_SECRET: PAIR_SECRET,
    });
    mock.seen.length = 0;
    const pairProbe = await client.connectors.test(pair.id);
    check("demo-pair: probe succeeds", pairProbe.success, JSON.stringify(pairProbe));
    const pairSeen = mock.seen.filter((entry) => entry.method === "tools/list");
    check(
      "demo-pair: both secrets resolve, into the headers they were bound to",
      pairSeen.length > 0 &&
        pairSeen.every(
          (entry) =>
            entry.authorization === `Bearer ${PAIR_SECRET}` && entry.clientId === PAIR_ID,
        ),
      JSON.stringify(mock.seen),
    );

    // The developer's own server, through the same two calls — which is the whole
    // point of §6.5: no extra surface, no marketplace.
    const dev = byName("dev-owned-mcp");
    const devSet = await client.connectors.setCredentials(dev.id, { DEV_KEY });
    check(
      "dev-owned-mcp: set configures the registered server",
      devSet.status === "configured" && devSet.missing.length === 0,
      JSON.stringify(devSet),
    );
    mock.seen.length = 0;
    const devProbe = await client.connectors.test(dev.id);
    check("dev-owned-mcp: probe succeeds", devProbe.success, JSON.stringify(devProbe));
    const devSeen = mock.seen.filter((entry) => entry.method === "tools/list");
    check(
      "dev-owned-mcp: the developer's key goes out as the resolved template",
      devSeen.length > 0 && devSeen.every((entry) => entry.authorization === `Bearer ${DEV_KEY}`),
      JSON.stringify(mock.seen),
    );
    const devCleared = await client.connectors.clearCredentials(dev.id);
    check(
      "dev-owned-mcp: clear returns it to requires_input",
      devCleared.status === "requires_input" && devCleared.missing.includes("DEV_KEY"),
      JSON.stringify(devCleared),
    );

    // ---- 3. clear puts it back to unconfigured, and back to silent --------
    const mixedCleared = await client.connectors.clearCredentials(mixed.id);
    check(
      "demo-mixed: clear returns it to requires_input",
      mixedCleared.status === "requires_input" &&
        mixedCleared.missing.includes("DEMO_API_KEY"),
      JSON.stringify(mixedCleared),
    );
    mock.seen.length = 0;
    const afterClear = await client.connectors.test(mixed.id);
    check(
      "demo-mixed: the probe refuses again after clear",
      !afterClear.success && afterClear.code === "MCP_MISSING_CREDENTIAL",
      JSON.stringify(afterClear),
    );
    check(
      "demo-mixed: and the mock receives nothing again",
      mock.seen.length === 0,
      JSON.stringify(mock.seen),
    );

    // The other connector's credential is untouched by that clear: one
    // connector's form does not reach into another's entry.
    const pairAfter = (await client.connectors.list()).find((c) => c.name === "demo-pair");
    check(
      "demo-pair: clearing demo-mixed left its own credential alone",
      pairAfter?.credential?.status === "configured",
      JSON.stringify(pairAfter?.credential),
    );

    // ---- cleanup ----------------------------------------------------------
    for (const entry of ["demo-mixed", "demo-pair"]) {
      const found = (await client.store.search(entry)).filter(
        (item) => item.marketplace_id === added.marketplace_id,
      );
      if (found[0]) await client.store.uninstall(found[0]);
    }
    ok("cleanup: both entries uninstalled");
    // The market too, so the script leaves the host as it found it and a second run
    // is not reading two of everything.
    await client.removeMarketplace(added.marketplace_id, true);
    ok("cleanup: the market is removed");
  } finally {
    client.close();
    mock.server.stop(true);
    rmSync(marketRoot, { recursive: true, force: true });
  }

  if (failures > 0) {
    console.error(`\n${failures} assertion(s) failed`);
    process.exit(1);
  }
  console.log("\nall connector-credential live assertions held");
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`live acceptance could not run: ${message}`);
  console.error(
    "\nThis script needs a live Agent Store host in local mode with a matching protocol fingerprint.\n" +
      "Start one with: target/debug/agent-store.exe --port 8903 --no-open --data-dir <scratch>",
  );
  process.exit(1);
});
