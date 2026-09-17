#!/usr/bin/env node
/**
 * Ad-hoc runtime probe for the Agent Store App Server (WS methods).
 *
 * Connects to ws://127.0.0.1:8787/api/app-server/ws, initializes, then calls
 * the read-side catalogs (agent/list, team/list, skill/list, connector/list)
 * and prints concise summaries. Dev-time verification only.
 *
 * Usage: node scripts/probe-agent-store-runtime.mjs [wsUrl]
 */

const wsUrl = process.argv[2] || "ws://127.0.0.1:8787/api/app-server/ws";
// Keep in sync with `nomifun_app_server::PROTOCOL_VERSION`. This was the one
// landing point `check-protocol-fingerprint.mjs` could not see (it was still on
// `2026-09-14` two shapes later); it is listed in the guard's `MIRRORS` now, so
// a future bump fails loudly here instead of leaving a probe that cannot connect.
const PROTOCOL_VERSION = process.argv[3] || "fp-7";

function connect(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.onopen = () => resolve(socket);
    socket.onerror = (event) => reject(new Error(`ws error: ${event?.message ?? "unknown"}`));
  });
}

let nextId = 1;
function rpc(socket, method, params) {
  const id = `probe-${nextId++}`;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${method}: timeout`)), 30000);
    const onMessage = (event) => {
      let payload;
      try {
        payload = JSON.parse(typeof event.data === "string" ? event.data : "{}");
      } catch {
        return;
      }
      if (payload.id !== id) return;
      clearTimeout(timer);
      socket.removeEventListener("message", onMessage);
      if (payload.error) reject(new Error(`${method}: ${JSON.stringify(payload.error)}`));
      else resolve(payload.result);
    };
    socket.addEventListener("message", onMessage);
    socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params: params ?? {} }));
  });
}

const summarize = (label, list, fields) => {
  const items = Array.isArray(list) ? list : list?.items ?? [];
  console.log(`\n=== ${label}: ${items.length} ===`);
  for (const item of items) {
    console.log("  " + fields.map((f) => `${f}=${JSON.stringify(item[f])}`).join(" | "));
  }
};

const main = async () => {
  const socket = await connect(wsUrl);
  const init = await rpc(socket, "initialize", {
    protocol_version: PROTOCOL_VERSION,
    client: { name: "runtime-probe", version: "1.0" },
  });
  console.log(`initialize ok: protocol=${init.protocol_version}`);
  // The App Server protocol requires an explicit `initialized` notification
  // (same as the HTTP handshake) before business methods are accepted.
  await rpc(socket, "initialized", {});
  console.log("initialized notification accepted");

  const agents = await rpc(socket, "agent/list", {});
  summarize("agent/list", agents, ["id", "name", "source", "enabled"]);

  const teams = await rpc(socket, "team/list", {});
  summarize("team/list", teams, ["id", "name", "source", "enabled"]);

  const skills = await rpc(socket, "skill/list", {});
  summarize("skill/list", skills, ["id", "name", "source", "enabled"]);

  const connectors = await rpc(socket, "connector/list", {});
  summarize("connector/list", connectors, ["id", "name", "kind", "enabled", "status"]);

  socket.close();
};

main().catch((error) => {
  console.error(`probe failed: ${error.message}`);
  process.exit(1);
});
