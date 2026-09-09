/** WP-3 P0-A live 验收：run/cancel（TC-RT-004）、版本冻结（TC-RT-002）、
 * 规范化审计（TC-RT-010）。协议面全部经 SDK client；provider 注册是宿主
 * admin 操作（标注 `[host admin]`）。
 *
 * 用法：AGENT_STORE_BIN=.../agent-store.exe bun scripts/sdk-live-p0a.ts
 * 模型 key 只读自 Hermes attachments config，仅内存持有、不打印。
 */
import { launchClient } from "@flowy-agent-store/sdk";
import { launchRun } from "@flowy-agent-store/client";
import { cpSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SOFTWARE_COMPANY = join(REPO_ROOT, "crates/backend/nomifun-importer/tests/fixtures/software-company");
const HERMES_CONFIG = join(process.env.LOCALAPPDATA ?? "", "hermes/attachments/config.toml");
const AGENT_MENTION = "wb-software-company-software-architect";
const TERMINAL = new Set(["completed", "completed_with_failures", "failed", "cancelled"]);

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

const launched = await launchClient({
  requestTimeoutMs: 600_000,
  ...(process.env.CHAIN_KEEP_DATA === "1"
    ? { dataDir: join(tmpdir(), `agent-store-p0a-${Date.now()}`) }
    : {}),
  client: { name: "sdk-live-p0a", version: "1" },
});
const { server, client } = launched;
const base = `http://${server.readiness.host}:${server.readiness.port}`;
console.log(`LISTENING ${base} data=${server.dataDir}`);

try {
  const { apiKey, baseUrl } = await readMimoCreds();
  await adminPost(base, "/api/providers", {
    platform: "openai",
    name: "mimo-p0a",
    base_url: baseUrl,
    api_key: apiKey,
    models: ["mimo-v2.5"],
    enabled: true,
  });

  // ---- setup: install the fixture agent (mention target) ----
  const agentImport = await client.runImport({ source_path: SOFTWARE_COMPANY, source_kind: "codebuddy-plugin" });
  check("P0A.setup.import", agentImport.status === "completed", agentImport.status);
  const agentInstall = await client.runInstall({ snapshot_id: agentImport.snapshot_id });
  check("P0A.setup.install", agentInstall.installed_count > 0, agentInstall.installed_count);
  const agents = await client.agents.list();
  const expert = agents.find((agent) => agent.id === AGENT_MENTION);
  check("P0A.setup.agent-visible", Boolean(expert?.preset_id), expert?.preset_id);
  check(
    "P0A.setup.preset-is-not-runtime-agent-id",
    Boolean(expert?.preset_id) && expert?.preset_id !== expert?.id,
    { preset_id: expert?.preset_id, agent_id: expert?.id },
  );

  // ================= TC-RT-004 run/cancel =================
  const cancelHandle = await launchRun(client.runs, {
    agentId: "",
    goal: "逐条列出 40 个 Python 标准库模块，每个写一句用途说明。",
    mentions: [{ kind: "agent", id: AGENT_MENTION }],
    idempotencyKey: `p0a-cancel-${Date.now()}`,
  });
  const observed: string[] = [];
  let view = await client.runs.get(cancelHandle.runId);
  const preCancelVersion = view.version;
  observed.push(`${view.status}@v${view.version}`);
  const cancelDeadline = Date.now() + 30_000;
  let cancelError = "";
  while (!TERMINAL.has(view.status) && Date.now() < cancelDeadline) {
    try {
      await client.runs.cancel({ runId: cancelHandle.runId, expectedVersion: view.version });
      observed.push("cancel-accepted");
      break;
    } catch (error) {
      cancelError = String(error).slice(0, 160);
      await Bun.sleep(150);
      view = await client.runs.get(cancelHandle.runId);
      observed.push(`${view.status}@v${view.version}`);
    }
  }
  check(
    "TC-RT-004.pre-cancel-non-terminal",
    observed.length > 0 && !TERMINAL.has(observed[0].split("@")[0]),
    observed,
  );
  const finalDeadline = Date.now() + 90_000;
  while (!TERMINAL.has(view.status) && Date.now() < finalDeadline) {
    await Bun.sleep(500);
    view = await client.runs.get(cancelHandle.runId);
  }
  check("TC-RT-004.terminal-cancelled", view.status === "cancelled", {
    status: view.status,
    observed,
    cancelError,
  });
  // Executions start at v0 (creation); every transition (including the cancel
  // CAS) bumps the version, so the terminal view must be strictly ahead of
  // the version the cancel command carried.
  check("TC-RT-004.version-progressed", view.version > preCancelVersion, {
    preCancelVersion,
    final: view.version,
  });
  await cancelHandle.close();

  // ================= TC-RT-002 版本冻结 =================
  const freezeInput = {
    agentId: "",
    goal: "用不少于 300 字说明软件架构评审的要点。",
    mentions: [{ kind: "agent" as const, id: AGENT_MENTION }],
    idempotencyKey: `p0a-freeze-${Date.now()}`,
  };
  const freezeHandle = await launchRun(client.runs, freezeInput);
  let freezeView = await client.runs.get(freezeHandle.runId);
  const frozen = {
    preset_revision: freezeView.preset_revision ?? null,
    content_digest: freezeView.content_digest ?? null,
  };
  check(
    "TC-RT-002.frozen-fields-present",
    frozen.preset_revision !== null && Boolean(frozen.content_digest),
    frozen,
  );

  // Publish a new version of the *same* agent while the run is in flight.
  const bumpedRoot = mkdtempSync(join(tmpdir(), "software-company-v2-"));
  cpSync(SOFTWARE_COMPANY, bumpedRoot, { recursive: true });
  const manifestPath = join(bumpedRoot, ".codebuddy-plugin/plugin.json");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as { version: string };
  manifest.version = "9.9.9";
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
  const bumpImport = await client.runImport({ source_path: bumpedRoot, source_kind: "codebuddy-plugin" });
  check("TC-RT-002.reimport-new-version", bumpImport.status === "completed", bumpImport.status);
  const bumpInstall = await client.runInstall({ snapshot_id: bumpImport.snapshot_id });
  check("TC-RT-002.reinstall-new-version", bumpInstall.installed_count > 0, bumpInstall.installed_count);

  freezeView = await client.runs.get(freezeHandle.runId);
  const after = {
    preset_revision: freezeView.preset_revision ?? null,
    content_digest: freezeView.content_digest ?? null,
  };
  check(
    "TC-RT-002.in-flight-fields-frozen",
    after.preset_revision === frozen.preset_revision && after.content_digest === frozen.content_digest,
    { before: frozen, after },
  );

  const freezeFinal = await freezeHandle.finished;
  await freezeHandle.close();
  check("TC-RT-002.run-completes", freezeFinal.status === "completed", freezeFinal.status);
  const freezeResult = await client.runs.result(freezeHandle.runId).catch(() => null);
  check(
    "TC-RT-002.result-fields-frozen",
    (freezeResult?.preset_revision ?? null) === frozen.preset_revision &&
      (freezeResult?.content_digest ?? null) === frozen.content_digest,
    { frozen, result: { preset_revision: freezeResult?.preset_revision, content_digest: freezeResult?.content_digest } },
  );

  // ================= TC-API-002 幂等重放 =================
  const replay = await client.runs.agent(freezeInput);
  check("TC-API-002.replay-same-run", replay.run_id === freezeHandle.runId, {
    first: freezeHandle.runId,
    replay: replay.run_id,
  });
  try {
    await client.runs.agent({ ...freezeInput, goal: `${freezeInput.goal}（改）` });
    check("TC-API-002.conflict-on-different-payload", false, "same key + different payload must conflict");
  } catch (error) {
    const code = (error as { code?: string }).code;
    check("TC-API-002.conflict-on-different-payload", code === "idempotency_conflict", { code });
  }

  // ================= TC-API-003 终态一致 =================
  const terminalView = await client.runs.get(freezeHandle.runId);
  const terminalResult = await client.runs.result(freezeHandle.runId);
  check(
    "TC-API-003.get-result-consistent",
    terminalView.status === terminalResult.status && terminalView.version === terminalResult.version,
    {
      get: { status: terminalView.status, version: terminalView.version },
      result: { status: terminalResult.status, version: terminalResult.version },
    },
  );

  // ================= TC-RT-010 规范化审计 =================
  const events = await client.runs.events({ runId: freezeHandle.runId, afterSequence: 0, limit: 500 });
  const installStatus = await client.getInstallStatus(agentImport.snapshot_id).catch(() => null);
  const auditTargets: Array<[string, unknown]> = [
    ["run/get", freezeView],
    ["run/result", freezeResult],
    ["run/events", events],
    ["store/install-entry", bumpInstall],
    ["install/status", installStatus],
    ["agents/list", agents],
  ];
  const forbidden = [apiKey, "Bearer ", "0190f5fe-"];
  const leaks: string[] = [];
  for (const [label, payload] of auditTargets) {
    const text = JSON.stringify(payload ?? "");
    for (const needle of forbidden) {
      if (needle && text.includes(needle)) leaks.push(`${label} :: ${needle.slice(0, 10)}…`);
    }
  }
  check("TC-RT-010.no-credentials-or-internal-ids", leaks.length === 0, leaks);

  try {
    await client.runs.cancel({ runId: "01a00000-0000-7000-8000-000000000000", expectedVersion: 1 });
    check("TC-RT-010.stable-error-code", false, "unknown-run cancel must fail");
  } catch (error) {
    const code = (error as { code?: string }).code;
    check("TC-RT-010.stable-error-code", typeof code === "string" && code.length > 0, {
      code,
      message: String(error).slice(0, 120),
    });
  }
} catch (error) {
  console.error("FAIL:", String(error).slice(0, 600));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${server.dataDir}`);
  await server.close();
  process.exit(failures === 0 ? 0 : 1);
}
