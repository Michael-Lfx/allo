/** WP-3 P0-B live 验收：TC-RT-005 重启恢复 + TC-RT-006 事件序。
 *
 * 脚本自持 agent-store 进程（硬杀模拟崩溃），协议面仍走 SDK client
 * （`AppServerClient` + `WebSocketTransport` 连回环 WS）；provider 注册是
 * 宿主 admin 操作（`[host admin]`）。
 *
 * 用法：AGENT_STORE_BIN=.../agent-store.exe bun scripts/sdk-live-p0b.ts
 */
import { AppServerClient, WebSocketTransport } from "@flowy-agent-store/client";
import { parseReadinessLine, resolveAppServerBin, type ReadinessInfo } from "@flowy-agent-store/sdk";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SOFTWARE_COMPANY = join(REPO_ROOT, "crates/backend/nomifun-importer/tests/fixtures/software-company");
const HERMES_CONFIG = join(process.env.LOCALAPPDATA ?? "", "hermes/attachments/config.toml");
const AGENT_MENTION = "wb-software-company-software-architect";
const TERMINAL = new Set(["completed", "completed_with_failures", "failed", "cancelled", "recovery_required"]);
const BIN = resolveAppServerBin();

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 320)}`;
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

interface ServerHandle {
  child: Bun.Subprocess;
  readiness: ReadinessInfo;
  client: AppServerClient;
  base: string;
}

/** Spawn the runtime and connect a fresh SDK client. stdout/stderr are drained
 * to files continuously (a stopped reader would back-pressure the server). */
async function startServer(dataDir: string, tag: string): Promise<ServerHandle> {
  const logPath = join(dataDir, `p0b-${tag}.log`);
  const child = Bun.spawn(
    [BIN, "--host", "127.0.0.1", "--port", "0", "--data-dir", dataDir, "--no-open"],
    { stdout: "pipe", stderr: "pipe", windowsHide: true },
  );
  let resolveReadiness: (info: ReadinessInfo) => void = () => undefined;
  const readiness = new Promise<ReadinessInfo>((resolvePromise) => {
    resolveReadiness = resolvePromise;
  });
  const drain = async (stream: ReadableStream<Uint8Array> | undefined, file: string, scan: boolean) => {
    if (!stream) return;
    const writer = Bun.file(file).writer();
    const decoder = new TextDecoder();
    let buffer = "";
    try {
      const reader = stream.getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        const text = decoder.decode(value, { stream: true });
        writer.write(text);
        if (!scan) continue;
        buffer += text;
        let index;
        while ((index = buffer.indexOf("\n")) >= 0) {
          const line = buffer.slice(0, index).trim();
          buffer = buffer.slice(index + 1);
          const info = parseReadinessLine(line);
          if (info) resolveReadiness(info);
        }
      }
    } finally {
      await writer.end();
    }
  };
  void drain(child.stdout as ReadableStream<Uint8Array>, logPath, true);
  void drain(child.stderr as ReadableStream<Uint8Array>, `${logPath}.err`, false);

  const info = await Promise.race([
    readiness,
    Bun.sleep(120_000).then(() => {
      throw new Error("readiness timeout");
    }),
  ]);
  const wsUrl = `ws://${info.host}:${info.port}/api/app-server/ws`;
  const client = new AppServerClient({
    transport: new WebSocketTransport(wsUrl, { requestTimeoutMs: 600_000 }),
    client: { name: "sdk-live-p0b", version: "1" },
  });
  await client.connect();
  return { child, readiness: info, client, base: `http://${info.host}:${info.port}` };
}

const dataDir = await mkdtemp(join(tmpdir(), "agent-store-p0b-"));
console.log(`DATA ${dataDir}`);

let first = await startServer(dataDir, "before");
let second: ServerHandle | null = null;
try {
  const { apiKey, baseUrl } = await readMimoCreds();
  await adminPost(first.base, "/api/providers", {
    platform: "openai",
    name: "mimo-p0b",
    base_url: baseUrl,
    api_key: apiKey,
    models: ["mimo-v2.5"],
    enabled: true,
  });

  const agentImport = await first.client.runImport({ source_path: SOFTWARE_COMPANY, source_kind: "codebuddy-plugin" });
  check("P0B.setup.import", agentImport.status === "completed", agentImport.status);
  const agentInstall = await first.client.runInstall({ snapshot_id: agentImport.snapshot_id });
  check("P0B.setup.install", agentInstall.installed_count > 0, agentInstall.installed_count);

  const receipt = await first.client.runs.agent({
    agentId: "",
    goal: "用不少于 400 字逐条说明微服务拆分的权衡，每条展开分析。",
    mentions: [{ kind: "agent", id: AGENT_MENTION }],
    idempotencyKey: `p0b-restart-${Date.now()}`,
  });
  const runId = receipt.run_id;

  // Wait until the run is actually running and has persisted a few events.
  let view = await first.client.runs.get(runId);
  const waitDeadline = Date.now() + 30_000;
  while (Date.now() < waitDeadline && view.status !== "running") {
    await Bun.sleep(300);
    view = await first.client.runs.get(runId);
  }
  const preKillEvents = await first.client.runs.events({ runId, afterSequence: 0, limit: 500 });
  check("P0B.pre-kill.running", view.status === "running", { status: view.status, version: view.version });
  check("P0B.pre-kill.events-exist", preKillEvents.length > 0, preKillEvents.map((event) => `${event.sequence}:${event.event_type}`));

  // ---- hard kill (crash simulation) ----
  first.child.kill(9);
  await first.child.exited;
  console.log(`KILLED pid=${first.child.pid}`);
  first.client.close();

  // ---- restart on the same data dir ----
  second = await startServer(dataDir, "after");
  let postView = await second.client.runs.get(runId);
  check("TC-RT-005.run-resolvable-after-restart", Boolean(postView.run_id), postView.status);
  // Anti-fake: a crash must never surface as a completed run.
  check("TC-RT-005.not-fake-completed", postView.status !== "completed", postView.status);

  const postEvents = await second.client.runs.events({ runId, afterSequence: 0, limit: 1000 });
  const preSequences = preKillEvents.map((event) => event.sequence);
  const postSequences = postEvents.map((event) => event.sequence);
  const preserved = preSequences.every((sequence) => postSequences.includes(sequence));
  check("TC-RT-006.events-preserved", preserved, { pre: preSequences, post: postSequences });
  const gapless = postSequences.every((sequence, index) => sequence === index + 1);
  check("TC-RT-006.sequence-gapless", gapless && postSequences.length > 0, postSequences);
  const recoveryEvents = postEvents.filter((event) =>
    JSON.stringify(event.payload ?? {}).includes("process_restart"),
  );
  console.log(
    `INFO recovery-events=${JSON.stringify(recoveryEvents.map((event) => `${event.sequence}:${event.event_type}`))}`,
  );

  // ---- settle: the recovery path must reach a safe terminal state ----
  const settleDeadline = Date.now() + 120_000;
  while (!TERMINAL.has(postView.status) && Date.now() < settleDeadline) {
    await Bun.sleep(1_000);
    postView = await second.client.runs.get(runId);
  }
  check("TC-RT-005.settles-safely", TERMINAL.has(postView.status), {
    status: postView.status,
    version: postView.version,
  });
  if (postView.status === "completed") {
    // A genuine resume must have written new events after the restart.
    const afterRestart = await second.client.runs.events({ runId, afterSequence: preSequences.at(-1) ?? 0, limit: 1000 });
    check("TC-RT-005.resume-has-new-events", afterRestart.length > 0, afterRestart.map((event) => event.event_type));
  }
  const finalEvents = await second.client.runs.events({ runId, afterSequence: 0, limit: 2000 });
  check(
    "TC-RT-006.final-sequence-monotonic",
    finalEvents.every((event, index) => index === 0 || event.sequence > finalEvents[index - 1].sequence),
    finalEvents.map((event) => event.sequence),
  );
} catch (error) {
  console.error("FAIL:", String(error).slice(0, 600));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${dataDir}`);
  try {
    first.client.close();
  } catch {}
  try {
    second?.client.close();
  } catch {}
  try {
    if (first.child.exitCode === null) first.child.kill(9);
  } catch {}
  try {
    if (second && second.child.exitCode === null) second.child.kill(9);
  } catch {}
  process.exit(failures === 0 ? 0 : 1);
}
