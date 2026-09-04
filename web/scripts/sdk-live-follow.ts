/** Live event-stream verification (Codex parity): spawn a real runtime via the
 * SDK, run an Agent end to end, and assert pushed `event` notifications arrive
 * deduped with an advancing cursor — then terminal state via run/get.
 *
 * Usage: AGENT_STORE_BIN=C:/workspace/allo/target/debug/agent-store.exe \
 *   bun scripts/sdk-live-follow.ts
 *
 * Provider key is read from the Hermes attachments config into memory only
 * and never printed.
 */
import { launchClient } from "@agent-store/sdk";
import type { RunEvent } from "@agent-store/protocol";

const HERMES_CONFIG = "C:/Users/15165/AppData/Local/hermes/attachments/config.toml";
const FIXTURE = "C:/workspace/allo/crates/backend/nomifun-importer/tests/fixtures/software-company";
const AGENT_MENTION = "wb-software-company-software-architect";

async function readMimoCreds(): Promise<{ apiKey: string; baseUrl: string }> {
  const body = await Bun.file(HERMES_CONFIG).text();
  const section = body.split("[providers.mimo]")[1]?.split("\n[")[0] ?? "";
  const apiKey = section.match(/api_key\s*=\s*"([^"]+)"/)?.[1] ?? "";
  const baseUrl = section.match(/base_url\s*=\s*"([^"]+)"/)?.[1] ?? "";
  if (!apiKey || !baseUrl) throw new Error("mimo creds not found");
  return { apiKey, baseUrl };
}

async function postJson(base: string, path: string, body: unknown, connectionId?: string) {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (connectionId) headers["x-app-server-connection-id"] = connectionId;
  const response = await fetch(base + path, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${path} -> ${response.status} ${text.slice(0, 300)}`);
  const headersOut: Record<string, string> = {};
  response.headers.forEach((value, key) => { headersOut[key] = value; });
  return { json: text ? JSON.parse(text) : null, headers: headersOut };
}

const launched = await launchClient({
  client: { name: "sdk-live-follow", version: "1" },
});
const { server, client } = launched;
const base = `http://${server.readiness.host}:${server.readiness.port}`;
console.log(`LISTENING ${base}`);
try {
  const { apiKey, baseUrl } = await readMimoCreds();
  await postJson(base, "/api/providers", {
    platform: "openai", name: "mimo-live", base_url: baseUrl,
    api_key: apiKey, models: ["mimo-v2.5"], enabled: true,
  });
  console.log("OK provider");
  const connectionId = launched.initializeResult.connection_id;

  const imp = await client.runImport({ source_path: FIXTURE, source_kind: "codebuddy-plugin" });
  const snapshotId = imp.snapshot_id;
  console.log(`OK import snapshot=${snapshotId}`);
  await client.runInstall({ snapshot_id: snapshotId });
  console.log("OK install");
  void connectionId;

  const receipt = await client.runs.agent({
    agentId: "",
    goal: "用一句话介绍你自己（中文，不要调用任何工具）。",
    mentions: [{ kind: "agent", id: AGENT_MENTION }],
    idempotencyKey: "sdk-live-follow-001",
  });
  console.log(`RUN ${receipt.run_id} status=${receipt.status}`);
  void receipt;

  const pushed: RunEvent[] = [];
  const sub = await client.runs.follow(receipt.run_id);
  sub.onEvent((event) => pushed.push(event));
  sub.onResync((params) => console.log(`resync notice: ${params.reason}`));

  const deadline = Date.now() + 8 * 60 * 1000;
  let view = await client.runs.get(receipt.run_id);
  while (!["completed", "failed", "cancelled"].includes(view.status) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 10000));
    view = await client.runs.get(receipt.run_id);
    console.log(`poll status=${view.status} pushed=${pushed.length} cursor=${sub.lastSequence}`);
  }
  console.log(`FINAL status=${view.status} pushed=${pushed.length}`);
  const sequences = pushed.map((event) => event.sequence);
  const dupes = sequences.length - new Set(sequences).size;
  console.log(`dupes=${dupes} foreign=${pushed.filter((e) => e.run_id !== receipt.run_id).length}`);
  const assert = (name: string, cond: boolean) => {
    console.log(`${cond ? "ASSERT-OK" : "ASSERT-FAIL"} ${name}`);
    if (!cond) throw new Error(`assertion failed: ${name}`);
  };
  assert("terminal state", ["completed", "failed", "cancelled"].includes(view.status));
  assert("pushed events observed", pushed.length > 0);
  assert("no duplicate sequences", dupes === 0);
  assert("no foreign run events", pushed.every((e) => e.run_id === receipt.run_id));
  assert("cursor covers pushed events", sub.lastSequence >= Math.max(...sequences));
  await sub.close();
  console.log("LIVE-FOLLOW PASS");
} finally {
  await launched.close();
  console.log("closed");
}