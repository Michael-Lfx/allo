/** WP-2 四链路 live 验收：专家 / 技能 / 连接器 / 专家团 的「下载 → 安装 → 使用」
 * 全链路，协议操作全部经 SDK 公共面（launchClient + client.*）。
 *
 * 宿主管理面（provider 注册、MCP enable）不属于 App Server 协议，用本地
 * admin HTTP 完成，调用点均已标注 `[host admin]`。
 *
 * 用法：AGENT_STORE_BIN=.../agent-store.exe bun scripts/sdk-live-store-chain.ts
 * 模型 key 只读自 Hermes attachments config，仅内存持有、不打印。
 */
import { launchClient } from "@flowy-agent-store/sdk";
import { launchRun } from "@flowy-agent-store/client";
import { Database } from "bun:sqlite";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const FIXTURES = join(REPO_ROOT, "crates/backend/nomifun-importer/tests/fixtures");
const SOFTWARE_COMPANY = join(FIXTURES, "software-company");
const SKILL_MARKET = join(FIXTURES, "skill-market");
const HERMES_CONFIG = join(process.env.LOCALAPPDATA ?? "", "hermes/attachments/config.toml");
const AGENT_MENTION = "wb-software-company-software-architect";
const SKILL_NAME = "hello";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 300)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}

async function readMimoCreds(): Promise<{ apiKey: string; baseUrl: string }> {
  const body = await Bun.file(HERMES_CONFIG).text();
  const section = body.split("[providers.mimo]")[1]?.split("\n[")[0] ?? "";
  const apiKey = section.match(/api_key\s*=\s*"([^"]+)"/)?.[1] ?? "";
  const baseUrl = section.match(/base_url\s*=\s*"([^"]+)"/)?.[1] ?? "";
  if (!apiKey || !baseUrl) throw new Error("mimo creds not found in Hermes config");
  return { apiKey, baseUrl };
}

/** Default marketplace ids from the host config (`~/.agent-store/config.toml`).
 * The runtime auto-registers these on the first store call; the real-market
 * smoke asserts at least one of them actually mirrored into the store list. */
async function readDefaultMarketplaceIds(): Promise<string[]> {
  const file = Bun.file(join(homedir(), ".agent-store", "config.toml"));
  if (!(await file.exists())) return [];
  return [...(await file.text()).matchAll(/\[default_marketplaces\.([0-9a-z-]+)\]/g)].map(
    (match) => match[1],
  );
}

/** Host admin HTTP (not App Server protocol). */
async function adminPost(base: string, path: string, body: unknown): Promise<unknown> {
  const response = await fetch(base + path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`[host admin] ${path} -> ${response.status} ${text.slice(0, 200)}`);
  return text ? JSON.parse(text) : null;
}

/** Minimal MCP stdio server (newline-delimited JSON-RPC) for the connector chain. */
const MOCK_MCP_SOURCE = `
const send = (msg) => process.stdout.write(JSON.stringify(msg) + "\\n");
let buffer = "";
process.stdin.on("data", (chunk) => {
  buffer += chunk.toString();
  let index;
  while ((index = buffer.indexOf("\\n")) >= 0) {
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    if (message.method === "initialize") {
      send({ jsonrpc: "2.0", id: message.id, result: { protocolVersion: "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "four-chain-mock", version: "1.0.0" } } });
    } else if (message.method === "tools/list") {
      send({ jsonrpc: "2.0", id: message.id, result: { tools: [{ name: "echo", description: "Echo text back", inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] } }] } });
    } else if (message.method === "tools/call") {
      const text = message.params?.arguments?.text ?? "";
      send({ jsonrpc: "2.0", id: message.id, result: { content: [{ type: "text", text: "echo:" + text }], isError: false } });
    } else if (message.id !== undefined) {
      send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found: " + message.method } });
    }
  }
});
`;

// `CHAIN_KEEP_DATA=1` keeps the instance data dir on exit for forensics;
// the default owns a fresh temp dir and removes it on close.
const launched = await launchClient({
  // The runtime auto-mirrors the host default marketplaces (a public full
  // tree) on the first store call; the 30s default request timeout is too
  // tight for that cold path. 600s matches the server-side bound.
  requestTimeoutMs: 600_000,
  ...(process.env.CHAIN_KEEP_DATA === "1"
    ? { dataDir: join(tmpdir(), `agent-store-chain-${Date.now()}`) }
    : {}),
  client: { name: "sdk-live-store-chain", version: "1" },
});
const { server, client } = launched;
const base = `http://${server.readiness.host}:${server.readiness.port}`;
console.log(`LISTENING ${base} data=${server.dataDir}`);

try {
  // ---- [host admin] register the mimo provider for the run chains ----
  const { apiKey, baseUrl } = await readMimoCreds();
  await adminPost(base, "/api/providers", {
    platform: "openai",
    name: "mimo-chain",
    base_url: baseUrl,
    api_key: apiKey,
    models: ["mimo-v2.5"],
    enabled: true,
  });

  // ---- S2 public model directory (REQ-PAR-05b) ----
  const models = await client.models.list();
  check(
    "S2.models-listed",
    models.some((entry) => entry.model === "mimo-v2.5"),
    models.map((entry) => `${entry.provider_name}/${entry.model}`),
  );
  check(
    "S2.models-default-flagged",
    models.filter((entry) => entry.is_default).length === 1,
    models.filter((entry) => entry.is_default),
  );
  check("S2.models-no-credentials", !JSON.stringify(models).includes(apiKey), "scanned");

  // ============================ C1 专家 ============================
  const agentImport = await client.runImport({
    source_path: SOFTWARE_COMPANY,
    source_kind: "codebuddy-plugin",
  });
  check("C1.import", agentImport.status === "completed", agentImport.status);
  const agentInstall = await client.runInstall({ snapshot_id: agentImport.snapshot_id });
  check("C1.install", agentInstall.installed_count > 0, agentInstall.installed_count);

  const agents = await client.agents.list();
  const expert = agents.find((agent) => agent.id === AGENT_MENTION);
  check("C1.catalog-visible", Boolean(expert?.preset_id), expert ? { preset_id: expert.preset_id } : null);

  const c1 = await launchRun(client.runs, {
    agentId: "",
    goal: "用一句话说明你负责什么。",
    mentions: [{ kind: "agent", id: AGENT_MENTION }],
    idempotencyKey: `chain-c1-${Date.now()}`,
  });
  const c1Final = await c1.finished;
  await c1.close();
  check("C1.run-completed", c1Final.status === "completed", c1Final.status);
  // S3 TurnResult aggregation (REQ-PAR-05c): final text + event/item lists.
  check(
    "S3.turn-result-aggregated",
    Boolean(c1Final.final_response) && c1Final.events.length > 0 && c1Final.items.length > 0,
    {
      final_response: (c1Final.final_response ?? "").slice(0, 60),
      events: c1Final.events.length,
      items: c1Final.items.map((item) => item.kind),
    },
  );

  // ============================ C4 专家团 ============================
  // V1 口径：下载 → 安装 → 可见；运行时为 Phase 2（§12 门禁）。
  const teams = await client.teams.list();
  const team = teams.find((candidate) => candidate.lead_agent_id?.includes("software-team-lead"));
  check("C4.team-visible", Boolean(team), team ? { id: team.id, lead: team.lead_agent_id } : teams.map((t) => t.id));

  // ============================ C2 技能 ============================
  const skillImport = await client.runImport({
    source_path: SKILL_MARKET,
    source_kind: "workbuddy-skill-market",
  });
  check("C2.import", skillImport.status === "completed", skillImport.status);
  const skillInstall = await client.runInstall({ snapshot_id: skillImport.snapshot_id });
  check("C2.install", skillInstall.installed_count === 2, skillInstall.installed_count);

  const skills = await client.skills.list();
  const hello = skills.find((skill) => skill.id === SKILL_NAME || skill.name === SKILL_NAME);
  check("C2.skill-visible", Boolean(hello), hello?.id ?? skills.map((skill) => skill.id).slice(0, 10));

  const c2 = await launchRun(client.runs, {
    agentId: "",
    goal: "请按 hello 技能的指引打个招呼。",
    mentions: [
      { kind: "agent", id: AGENT_MENTION },
      { kind: "skill", id: SKILL_NAME },
    ],
    idempotencyKey: `chain-c2-${Date.now()}`,
  });
  const c2Final = await c2.finished;
  await c2.close();
  check("C2.run-completed", c2Final.status === "completed", c2Final.status);

  // 挂载证明：run 的新会话快照必须冻结该技能绑定（读本实例的临时库）。
  const db = new Database(join(server.dataDir, "flowy-backend.db"), { readonly: true });
  try {
    const rows = db
      .query("SELECT conversation_id, extra FROM conversations ORDER BY id DESC LIMIT 5")
      .all() as Array<{ conversation_id: string; extra: string }>;
    const frozen = rows.some((row) => {
      const parsed = JSON.parse(row.extra ?? "{}") as { skills?: string[] };
      return (parsed.skills ?? []).some((entry) => entry.includes(SKILL_NAME));
    });
    check("C2.skill-frozen-in-conversation", frozen, rows.map((row) => row.conversation_id));
  } finally {
    db.close();
  }

  // ============================ C3 连接器 ============================
  // The marketplace id derives from the source directory basename and must be
  // lowercase (`[0-9a-z-]`), so stage the market under a fixed lowercase dir.
  const marketRoot = await mkdtemp(join(tmpdir(), "four-chain-market-"));
  const marketDir = join(marketRoot, "four-chain-market");
  const mockScript = join(marketRoot, "mock-mcp.mjs");
  await writeFile(mockScript, MOCK_MCP_SOURCE);
  await mkdir(join(marketDir, ".codebuddy-connector"), { recursive: true });
  await mkdir(join(marketDir, "connectors/four-chain-mock"), { recursive: true });
  await mkdir(join(marketDir, "connectors/four-chain-broken"), { recursive: true });
  await writeFile(
    join(marketDir, ".codebuddy-connector/connectors.json"),
    JSON.stringify({
      name: "four-chain-connectors",
      connectors: [
        {
          id: "four-chain-mock",
          name: "FourChainMock",
          version: "1.0.0",
          description: "Local mock MCP for the four-chain live check",
          type: "mcp",
        },
        {
          id: "four-chain-broken",
          name: "FourChainBroken",
          version: "1.0.0",
          description: "MCP entry whose probe must fail (TC-CONN-002)",
          type: "mcp",
        },
      ],
    }),
  );
  await writeFile(
    join(marketDir, "connectors/four-chain-mock/mcp.json"),
    JSON.stringify({
      mcpServers: {
        "four-chain-mock": { type: "stdio", command: process.execPath, args: [mockScript] },
      },
    }),
  );
  await writeFile(
    join(marketDir, "connectors/four-chain-broken/mcp.json"),
    JSON.stringify({
      mcpServers: {
        "four-chain-broken": {
          type: "stdio",
          command: "four-chain-missing-command",
          args: ["--never-starts"],
        },
      },
    }),
  );

  const market = await client.addMarketplace({ source_kind: "directory", source: marketDir });
  const store = await client.listStore();
  // S1 real-market smoke: the host default sources must mirror into the store.
  const defaultMarketplaceIds = await readDefaultMarketplaceIds();
  if (defaultMarketplaceIds.length > 0) {
    const mirrored = new Set(store.items.map((item) => item.marketplace_id));
    const hit = defaultMarketplaceIds.filter((id) => mirrored.has(id));
    check("S1.real-market-mirrored", hit.length > 0, { configured: defaultMarketplaceIds, hit });
  } else {
    console.log("SKIP S1.real-market-mirrored :: no default marketplaces configured");
  }
  const storeItem = store.items.find((item) => item.entry_name === "four-chain-mock");
  check("C3.store-visible", Boolean(storeItem), storeItem?.entry_name);

  const connectorInstall = await client.installStoreEntry(market.marketplace_id, "four-chain-mock");
  check("C3.install", connectorInstall.installed_count > 0, connectorInstall.installed_count);

  const connectors = await client.connectors.list();
  const connector = connectors.find((entry) => entry.name === "FourChainMock" || entry.name === "four-chain-mock");
  check(
    "C3.catalog-visible",
    Boolean(connector),
    connectors.map((entry) => ({ id: entry.id, name: entry.name, enabled: entry.enabled })),
  );

  if (connector) {
    // [host admin] MCP enable/disable is a host config action, not a protocol method.
    await adminPost(base, `/api/mcp/servers/${connector.id}/toggle`, {});
    const enabled = (await client.connectors.list()).find((entry) => entry.id === connector.id)?.enabled === true;
    check("C3.enabled", enabled);

    const probe = await client.connectors.test(connector.id);
    const tools = (probe.tools ?? []).map((tool) => tool.name);
    check("C3.tool-listing", probe.success && tools.includes("echo"), {
      success: probe.success,
      tools,
      error: probe.error,
    });

    const c3 = await launchRun(client.runs, {
      agentId: "",
      goal: "调用 echo 工具，参数 text 为 four-chain，并把工具返回内容原样报告。",
      mentions: [
        { kind: "agent", id: AGENT_MENTION },
        { kind: "connector", id: connector.id },
      ],
      idempotencyKey: `chain-c3-${Date.now()}`,
    });
    check("C3.mention-accepted", Boolean(c3.runId), c3.runId);
    const c3Final = await c3.finished;
    const c3Result = await client.runs.result(c3.runId).catch(() => null);
    await c3.close();
    check("C3.run-completed", c3Final.status === "completed", c3Final.status);
    // 真实调用烟测（best effort）：模型报告里出现 mock 的 echo 前缀。
    const summary = JSON.stringify(c3Result?.summary ?? "");
    check("C3.tool-called", summary.includes("echo:"), summary.slice(0, 200));

    // TC-CONN-002: a configured connector whose probe fails must not report
    // connected.
    const brokenInstall = await client.installStoreEntry(market.marketplace_id, "four-chain-broken");
    check("TC-CONN-002.install", brokenInstall.installed_count > 0, brokenInstall.installed_count);
    const broken = (await client.connectors.list()).find(
      (entry) => entry.name === "four-chain-broken" || entry.name === "FourChainBroken",
    );
    if (broken) {
      await adminPost(base, `/api/mcp/servers/${broken.id}/toggle`, {});
      const brokenProbe = await client.connectors.test(broken.id).catch((error) => ({
        connector_id: broken.id,
        success: false,
        error: String(error),
      }));
      const brokenStatus = await client.connectors.status(broken.id).catch(() => null);
      check(
        "TC-CONN-002.probe-failure-not-connected",
        !brokenProbe.success && brokenStatus?.status !== "connected",
        { probe_success: brokenProbe.success, status: brokenStatus?.status, error: brokenProbe.error },
      );
    } else {
      check("TC-CONN-002.catalog-visible", false, "broken connector not registered");
    }
  }
} catch (error) {
  console.error("FAIL:", String(error).slice(0, 600));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
