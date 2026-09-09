/** Live AgentRunHandle verification (Codex parity): spawn a real runtime via
 * the SDK, launch a run through the high-level handle, iterate the live event
 * stream with `for await`, then await `handle.finished` for the terminal view.
 *
 * Usage: AGENT_STORE_BIN=C:/workspace/allo/target/debug/agent-store.exe \
 *   bun scripts/sdk-live-handle.ts
 *
 * Provider key is read from the Hermes attachments config into memory only
 * and never printed.
 */
import { launchClient } from "@flowy-agent-store/sdk";
import { launchRun } from "@flowy-agent-store/client";

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
  return text ? JSON.parse(text) : null;
}

const launched = await launchClient({
  client: { name: "sdk-live-handle", version: "1" },
});
const { server, client } = launched;
const base = `http://${server.readiness.host}:${server.readiness.port}`;
console.log(`LISTENING ${base}`);
try {
  const { apiKey, baseUrl } = await readMimoCreds();
  await postJson(base, "/api/providers", {
    platform: "openai", name: "mimo-handle", base_url: baseUrl,
    api_key: apiKey, models: ["mimo-v2.5"], enabled: true,
  });
  console.log("OK provider");
  void launched.initializeResult.connection_id;

  const imp = await client.runImport({ source_path: FIXTURE, source_kind: "codebuddy-plugin" });
  console.log(`OK import snapshot=${imp.snapshot_id}`);
  await client.runInstall({ snapshot_id: imp.snapshot_id });
  console.log("OK install");

  // Codex-parity shape: launch → iterate live events → await finished.
  const handle = await launchRun(client.runs, {
    agentId: "",
    goal: "用一句话介绍你自己（中文，不要调用任何工具）。",
    mentions: [{ kind: "agent", id: AGENT_MENTION }],
    idempotencyKey: "sdk-live-handle-001",
  });
  console.log(`RUN ${handle.runId} status=${handle.receipt.status}`);

  const seen: Array<{ sequence: number; event_type: string }> = [];
  const consume = (async () => {
    for await (const event of handle) {
      seen.push({ sequence: event.sequence, event_type: event.event_type });
    }
  })();

  const finished = handle.finished;
  const view = await finished;
  console.log(`FINAL status=${view.status} events=${seen.length}`);
  // Drain anything still queued before closing.
  await new Promise((resolve) => setTimeout(resolve, 1500));
  await handle.close();
  await consume.catch(() => undefined);

  const sequences = seen.map((e) => e.sequence);
  const dupes = sequences.length - new Set(sequences).size;
  const assert = (name: string, cond: boolean) => {
    console.log(`${cond ? "ASSERT-OK" : "ASSERT-FAIL"} ${name}`);
    if (!cond) throw new Error(`assertion failed: ${name}`);
  };
  assert("terminal state", ["completed", "failed", "cancelled"].includes(view.status));
  assert("events observed", seen.length > 0);
  assert("no duplicate sequences", dupes === 0);
  console.log(`lastSeq=${handle.lastSequence} types=${JSON.stringify([...new Set(seen.map((e) => e.event_type))])}`);
  console.log("LIVE-HANDLE PASS");
} finally {
  await launched.close();
  console.log("closed");
}
