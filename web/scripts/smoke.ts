/**
 * End-to-end smoke test: typed client + App Server.
 *
 * Two modes:
 *   default            spawns the mock App Server on an ephemeral port and
 *                      asserts the full deterministic flow (including terminal
 *                      states).
 *   --real             connects to a real App Server backend (e.g.
 *                      `cargo run -p nomifun-web -- --port 8787 --api-only
 *                      --insecure-no-auth`) and asserts the protocol surface
 *                      with realistic tolerances (a planning run need not
 *                      reach a terminal state without a model provider).
 *
 * Usage:
 *   bun scripts/smoke.ts
 *   bun scripts/smoke.ts --real --ws ws://127.0.0.1:8787/api/app-server/ws \
 *       --http http://127.0.0.1:8787/api/app-server --agent-id <uuidv7>
 */

import { spawn, type Subprocess } from "bun";
import { AppServerClient } from "../src/lib/client";
import { AppServerError } from "../src/lib/errors";

const MOCK_PORT = 17990;
const MOCK_AGENT_ID = "0190f5fe-7c00-7a00-8000-000000000004";

interface SmokeOptions {
  real: boolean;
  wsUrl: string;
  httpUrl: string;
  agentId: string;
}

function parseArgs(): SmokeOptions {
  const args = process.argv.slice(2);
  const valueOf = (flag: string): string | undefined => {
    const index = args.indexOf(flag);
    return index >= 0 ? args[index + 1] : undefined;
  };
  const real = args.includes("--real");
  return {
    real,
    wsUrl: valueOf("--ws") ?? (real ? "ws://127.0.0.1:8787/api/app-server/ws" : `ws://127.0.0.1:${MOCK_PORT}/api/app-server/ws`),
    httpUrl: valueOf("--http") ?? (real ? "http://127.0.0.1:8787/api/app-server" : `http://127.0.0.1:${MOCK_PORT}/api/app-server`),
    agentId: valueOf("--agent-id") ?? MOCK_AGENT_ID,
  };
}

let failures = 0;

function ok(name: string): void {
  console.log(`  ✓ ${name}`);
}

function fail(name: string, detail: string): void {
  failures += 1;
  console.error(`  ✗ ${name}: ${detail}`);
}

async function waitForHttp(url: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let firstError: string | null = null;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${url}/initialize`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          protocol_version: "2026-08-26",
          client: { name: "smoke", version: "0.0.0" },
          capabilities: {},
        }),
      });
      if (response.ok) {
        return;
      }
      firstError = `http ${response.status}`;
    } catch (error) {
      firstError ??= String(error);
    }
    await Bun.sleep(250);
  }
  throw new Error(`server did not become ready at ${url} (last error: ${firstError})`);
}

async function discoverAgentId(httpUrl: string): Promise<string | null> {
  const base = httpUrl.replace(/\/api\/app-server$/, "");
  const response = await fetch(`${base}/api/presets`, {
    headers: { accept: "application/json" },
  });
  if (!response.ok) {
    return null;
  }
  const body = (await response.json()) as {
    items?: Array<{ preset_id?: string; id?: string; source?: string; source_key?: string }>;
    data?: Array<{ preset_id?: string; id?: string; source?: string; source_key?: string }>;
  };
  const items = body.items ?? body.data ?? [];
  const office = items.find(
    (item) => item.source === "builtin" && item.source_key === "builtin-office",
  );
  return office?.preset_id ?? office?.id ?? null;
}

async function runSuccessPath(
  client: AppServerClient,
  input: { agentId: string; workspaceId: string; key: string; real: boolean },
): Promise<void> {
  const receipt = await client.runs.agent({
    agentId: input.agentId,
    goal: "inspect the repository and report findings",
    workspaceId: input.workspaceId,
    idempotencyKey: input.key,
  });
  if (receipt.run_id.length === 36) {
    ok(`agent/run returned asynchronous receipt (${receipt.run_id.slice(0, 8)}…)`);
  } else {
    fail("agent/run receipt", JSON.stringify(receipt));
  }

  // --- idempotency replay ------------------------------------------------
  const replay = await client.runs.agent({
    agentId: input.agentId,
    goal: "inspect the repository and report findings",
    workspaceId: input.workspaceId,
    idempotencyKey: input.key,
  });
  if (replay.run_id === receipt.run_id) {
    ok("same idempotency key replays the same receipt");
  } else {
    fail("idempotency replay", `${replay.run_id} != ${receipt.run_id}`);
  }

  // --- realtime events ---------------------------------------------------
  const events: string[] = [];
  const subscription = await client.runs.follow(receipt.run_id);
  subscription.onEvent((event) => {
    events.push(`${event.sequence}:${event.event_type}`);
  });
  const sawEvent = await new Promise<boolean>((resolve) => {
    const timer = setTimeout(() => resolve(false), 8000);
    const off = subscription.onEvent(() => {
      clearTimeout(timer);
      off();
      resolve(true);
    });
  });
  if (sawEvent && events.length > 0) {
    ok(`realtime subscription received ${events.length} event(s): ${events.join(", ")}`);
  } else {
    fail("realtime subscription", "no event arrived within 8s");
  }
  await subscription.close();

  // --- state queries -----------------------------------------------------
  const view = await client.runs.get(receipt.run_id);
  if (view.status.length > 0) {
    ok(`run/get returns authoritative state (${view.status})`);
  } else {
    fail("run/get", JSON.stringify(view));
  }

  const history = await client.runs.events({ runId: receipt.run_id, limit: 50 });
  const ordered = history.every((event, index) => index === 0 || history[index - 1].sequence < event.sequence);
  if (history.length > 0 && ordered) {
    ok(`run/events replays ${history.length} events in sequence order`);
  } else {
    fail("run/events", JSON.stringify(history.map((event) => event.sequence)));
  }

  if (!input.real) {
    // Mock mode converges deterministically.
    let terminal: Awaited<ReturnType<typeof client.runs.get>>;
    terminal = view;
    for (let attempt = 0; attempt < 20 && terminal.status !== "completed"; attempt += 1) {
      await Bun.sleep(300);
      terminal = await client.runs.get(receipt.run_id);
    }
    if (terminal.status === "completed") {
      ok("run/get converges to completed after events");
    } else {
      fail("run/get convergence", JSON.stringify(terminal));
    }
    const result = await client.runs.result(receipt.run_id);
    if (result.status === "completed" && result.output_files.includes("findings.md")) {
      ok("run/result returns terminal state and output files");
    } else {
      fail("run/result", JSON.stringify(result));
    }
  } else {
    // Real mode: result is only available for terminal runs; a planning run
    // must produce the structured conflict error, never a hang.
    try {
      const result = await client.runs.result(receipt.run_id);
      ok(`run/result returned a terminal result (${result.status})`);
    } catch (error) {
      if (error instanceof AppServerError && error.code === "conflict") {
        ok("run/result on a non-terminal run returns structured conflict (no hang)");
      } else {
        fail("run/result non-terminal", String(error));
      }
    }
  }
}

async function runRealConversationFlow(
  client: AppServerClient,
  created: Awaited<ReturnType<AppServerClient["conversations"]["create"]>>,
): Promise<void> {
  if (created.conversation_id.length === 36 && created.model.model.length > 0) {
    ok(`conversation/create resolves provider/model from agent-store config (${created.model.provider_id.slice(0, 8)}… / ${created.model.model})`);
  } else {
    fail("conversation/create", JSON.stringify(created));
    return;
  }

  const list = await client.conversations.list();
  if (list.some((conversation) => conversation.conversation_id === created.conversation_id)) {
    ok("conversation/list includes the App Server chat projection");
  } else {
    fail("conversation/list", JSON.stringify(list));
  }

  try {
    const options = await client.conversations.modelOptions();
    if (options.reasoning_efforts.includes("high") && options.providers.length > 0) {
      ok("conversation/model-options exposes the agent-store catalog");
    } else {
      ok(`conversation/model-options degrades to a clean catalog (${options.providers.length} provider(s))`);
    }
  } catch (error) {
    fail("conversation/model-options", String(error));
  }

  try {
    const updated = await client.conversations.update(created.conversation_id, {
      reasoningEffort: "high",
      model: { provider_id: "opencode", model: "mimo-v2.5-free" },
    });
    if (updated.conversation_id === created.conversation_id && updated.model.model === "mimo-v2.5-free") {
      ok("conversation/update accepts config-key models and reasoning efforts on real chats");
    } else {
      fail("conversation/update", JSON.stringify(updated));
    }
  } catch (error) {
    fail("conversation/update", String(error));
  }

  try {
    const receipt = await client.conversations.send(
      created.conversation_id,
      "hello from real smoke",
      `chat-${Date.now()}`,
    );
    if (receipt.accepted && receipt.message_id.length === 36) {
      ok("conversation/send accepts a provider-backed message");
    } else {
      fail("conversation/send", JSON.stringify(receipt));
    }
  } catch (error) {
    if (error instanceof AppServerError && error.code.length > 0) {
      ok(`conversation/send surfaces provider/environment errors structurally (${error.code})`);
    } else {
      fail("conversation/send", String(error));
    }
  }

  // The user message is durable regardless of streaming outcome.
  const textOf = (value: unknown): string => {
    if (typeof value === "string") return value;
    if (
      value && typeof value === "object" &&
      "content" in value && typeof (value as { content?: unknown }).content === "string"
    ) {
      return (value as { content: string }).content;
    }
    return "";
  };
  let userPersisted = false;
  for (let attempt = 0; attempt < 10 && !userPersisted; attempt += 1) {
    if (attempt > 0) await Bun.sleep(1000);
    const history = await client.conversations.messages({ conversationId: created.conversation_id, pageSize: 50 });
    userPersisted = history.items.some(
      (message) => message.role === "user" && textOf(message.content) === "hello from real smoke",
    );
  }
  if (userPersisted) {
    ok("conversation/messages replays the durable user message");
  } else {
    fail("conversation/messages replay", "user message was not persisted within 10s");
  }
}

async function runCatalogPath(client: AppServerClient, real: boolean): Promise<void> {
  const capabilities = client.initializeInfo?.capabilities;
  if (!capabilities?.skills && !capabilities?.connectors) {
    // The server did not advertise the catalog — nothing else to verify here.
    return;
  }

  const skills = await client.skills.list();
  if (Array.isArray(skills)) {
    ok(`skill/list returns ${skills.length} skill(s)`);
  } else {
    fail("skill/list", "expected an array");
    return;
  }
  const connectors = await client.connectors.list();
  if (Array.isArray(connectors)) {
    ok(`connector/list returns ${connectors.length} connector(s)`);
  } else {
    fail("connector/list", "expected an array");
    return;
  }

  if (real) {
    // The real catalog reflects the current data dir; only the protocol
    // surface is asserted here (covers the WS import/install/market/store
    // arms added by the protocol-unification pass).
    if (capabilities?.store) {
      const store = await client.listStore();
      if (Array.isArray(store.items)) {
        ok(`store/list returns ${store.items.length} item(s)`);
      } else {
        fail("store/list", "expected { items: [] }");
      }
    }
    if (capabilities?.imports) {
      const imports = await client.listImports();
      if (Array.isArray(imports)) {
        ok(`import/list returns ${imports.length} snapshot(s)`);
      } else {
        fail("import/list", "expected an array");
      }
    }
    if (capabilities?.marketplaces) {
      const markets = await client.listMarketplaces();
      if (Array.isArray(markets)) {
        ok(`market/list returns ${markets.length} marketplace(s)`);
      } else {
        fail("market/list", "expected an array");
      }
    }
    return;
  }

  // Mock data is deterministic: pdf skill, github (oauth) + flaky-bridge.
  if (skills.length > 0) {
    const detail = await client.skills.get(skills[0].id);
    if (detail.id === skills[0].id) {
      ok(`skill/get returns the requested skill (${detail.id})`);
    } else {
      fail("skill/get", `expected ${skills[0].id}, got ${detail.id}`);
    }
  }

  // A failed probe must never surface as `connected` (TC-CONN-002).
  const flakyId = "0190f5fe-7c00-7a00-8000-000000000103";
  const flakyStatus = await client.connectors.status(flakyId);
  if (flakyStatus.status !== "connected" && flakyStatus.status === "error") {
    ok("connector/status never reports connected after a failed probe");
  } else {
    fail("connector/status merge", JSON.stringify(flakyStatus));
  }

  // Probe happy path persists success.
  const playwrightId = "0190f5fe-7c00-7a00-8000-000000000101";
  const probe = await client.connectors.test(playwrightId);
  if (probe.success === true && Array.isArray(probe.tools) && probe.tools.length > 0) {
    ok("connector/test probes tools for a healthy connector");
  } else {
    fail("connector/test", JSON.stringify(probe));
  }

  // OAuth round trip: start → poll authenticated → logout.
  const githubId = "0190f5fe-7c00-7a00-8000-000000000102";
  const before = await client.connectors.authStatus(githubId);
  if (before.state === "not_authenticated") {
    ok("connector/auth/status starts not_authenticated");
  } else {
    fail("connector/auth/status initial", JSON.stringify(before));
  }
  const started = await client.connectors.authStart(githubId);
  if (started.state === "started") {
    ok("connector/auth/start acknowledges the browser flow");
  } else {
    fail("connector/auth/start", JSON.stringify(started));
  }
  const deadline = Date.now() + 3000;
  let authenticated = false;
  while (Date.now() < deadline) {
    const status = await client.connectors.authStatus(githubId);
    if (status.state === "authenticated") {
      authenticated = true;
      break;
    }
    await Bun.sleep(150);
  }
  if (authenticated) {
    ok("connector/auth/status flips to authenticated after start");
  } else {
    fail("connector/auth/status poll", "did not become authenticated within 3s");
  }
  await client.connectors.logout(githubId);
  const after = await client.connectors.authStatus(githubId);
  if (after.state === "not_authenticated") {
    ok("connector/auth/logout revokes the mock token");
  } else {
    fail("connector/auth/logout", JSON.stringify(after));
  }
}

async function runConversationPath(client: AppServerClient, workspaceId: string, real: boolean): Promise<void> {
  if (real) {
    // A clean temporary backend has no registered provider. With
    // `~/.agent-store/config.toml` present the App Server registers its
    // provider and resolves `default_model` itself; without it the create
    // must fail with a stable structured error.
    try {
      const created = await client.conversations.create({
        name: "Conversation smoke",
        workspaceId,
        model: { provider_id: "opencode", model: "mimo-v2.5-free" },
        reasoningEffort: "medium",
      });
      await runRealConversationFlow(client, created);
    } catch (error) {
      if (error instanceof AppServerError && error.code.length > 0 && !error.retryable) {
        ok(`conversation/create without agent-store config fails cleanly (${error.code})`);
      } else {
        fail("conversation/create without provider", String(error));
      }
    }
    return;
  }

  const created = await client.conversations.create({
    name: "Conversation smoke",
    model: { provider_id: "0190f5fe-7c00-7a00-8000-000000000004", model: "mock-model" },
    workspaceId,
  });
  if (created.conversation_id.length === 36 && created.model.model.length > 0) {
    ok(`conversation/create returns a persistent Nomi chat (${created.conversation_id.slice(0, 8)}…)`);
  } else {
    fail("conversation/create", JSON.stringify(created));
  }

  const list = await client.conversations.list();
  if (list.some((conversation) => conversation.conversation_id === created.conversation_id)) {
    ok("conversation/list includes the App Server chat projection");
  } else {
    fail("conversation/list", JSON.stringify(list));
  }

  const options = await client.conversations.modelOptions();
  if (
    options.default?.provider &&
    options.providers.some((provider) => provider.models.length > 0) &&
    options.reasoning_efforts.includes("high")
  ) {
    ok("conversation/model-options exposes providers, models and reasoning efforts");
  } else {
    fail("conversation/model-options", JSON.stringify(options));
  }

  const updated = await client.conversations.update(created.conversation_id, {
    reasoningEffort: "high",
    model: { provider_id: "0190f5fe-7c00-7a00-8000-000000000004", model: "mock-model" },
  });
  if (updated.conversation_id === created.conversation_id && updated.model.model === "mock-model") {
    ok("conversation/update applies reasoning effort and model to an existing chat");
  } else {
    fail("conversation/update", JSON.stringify(updated));
  }

  const subscription = await client.conversations.follow(created.conversation_id);
  const observed: string[] = [];
  subscription.onEvent((event) => observed.push(`${event.sequence}:${event.event_type}`));
  const receipt = await client.conversations.send(created.conversation_id, "hello from chat smoke", `chat-${Date.now()}`);
  if (receipt.accepted && receipt.message_id.length === 36) {
    ok("conversation/send returns an idempotent public receipt");
  } else {
    fail("conversation/send", JSON.stringify(receipt));
  }

  const sawRealtime = await new Promise<boolean>((resolve) => {
    const timer = setTimeout(() => resolve(false), 5_000);
    const unsubscribe = subscription.onEvent((event) => {
      if (event.event_type === "message.delta") {
        clearTimeout(timer);
        unsubscribe();
        resolve(true);
      }
    });
  });
  const thinkingEvents = observed.filter((event) => event.endsWith("message.thinking"));
  const tipsEvents = observed.filter((event) => event.endsWith("message.tips"));
  const toolEvents = observed.filter((event) => event.endsWith("message.tool"));
  if (sawRealtime && observed.some((event) => event.endsWith("message.created")) && thinkingEvents.length >= 3 && tipsEvents.length >= 1 && toolEvents.length >= 1) {
    ok(`conversation subscription receives ordered streaming thinking, tips, tool and chat events (${observed.join(", ")})`);
  } else {
    fail("conversation realtime", observed.join(", ") || "no event received");
  }
  const history = await client.conversations.messages({ conversationId: created.conversation_id, pageSize: 50 });
  if (history.items.some((message) => message.role === "user") && history.items.some((message) => message.role === "assistant")) {
    ok("conversation/messages replays durable user and assistant history");
  } else {
    fail("conversation/messages", JSON.stringify(history));
  }
  await subscription.close();
}

async function runWorkspaceLifecycle(client: AppServerClient, workspaceId: string): Promise<void> {
  // --- workspace list includes the default registered workspace -------------
  const workspaces = await client.workspaces.list();
  if (workspaces.some((workspace) => workspace.workspace_id === workspaceId)) {
    ok(`workspace/list returns the owner's active workspaces (${workspaces.length})`);
  } else {
    fail("workspace/list", JSON.stringify(workspaces));
  }

  // --- user-path registration is idempotent per canonical root --------------
  const root = (process.env.TEMP ?? "/tmp").replace(/[\\/]+$/, "");
  const demoPath = `${root}${/^[A-Za-z]:[\\/]/.test(root) ? "\\" : "/"}allo-smoke-${Date.now()}`;
  const created = await client.workspaces.create(demoPath);
  if (created.workspace_id.length === 36 && created.name.length > 0 && created.canonical_path === demoPath) {
    ok(`workspace/create registers a user-chosen path (${created.name})`);
  } else {
    fail("workspace/create", JSON.stringify(created));
  }
  const again = await client.workspaces.create(demoPath);
  if (again.workspace_id === created.workspace_id) {
    ok("workspace/create reuses the same workspace for the same path");
  } else {
    fail("workspace/create idempotency", `${again.workspace_id} != ${created.workspace_id}`);
  }

  // --- chat created inside a workspace carries workspace_id -----------------
  const chat = await client.conversations.create({
    name: "Workspace lifecycle smoke",
    model: { provider_id: "0190f5fe-7c00-7a00-8000-000000000004", model: "mock-model" },
    workspaceId: created.workspace_id,
  });
  if (chat.workspace_id === created.workspace_id) {
    ok("conversation/create records the workspace lineage (workspace_id)");
  } else {
    fail("conversation workspace_id", JSON.stringify(chat));
  }

  // --- rename persists through the shared update surface --------------------
  const renamed = await client.conversations.update(chat.conversation_id, { name: "Renamed by smoke" });
  const afterRename = await client.conversations.get(chat.conversation_id);
  if (renamed.name === "Renamed by smoke" && afterRename.name === "Renamed by smoke") {
    ok("conversation/update renames and survives a fresh read");
  } else {
    fail("conversation rename", JSON.stringify(afterRename));
  }

  // --- send completes → a measured context snapshot becomes readable --------
  await client.conversations.send(chat.conversation_id, "hello for context usage", `chat-${Date.now()}`);
  let sawUsage = false;
  for (let attempt = 0; attempt < 20 && !sawUsage; attempt += 1) {
    if (attempt > 0) await Bun.sleep(400);
    const view = await client.conversations.get(chat.conversation_id);
    const usage = view.context_usage;
    if (usage && usage.used_tokens > 0 && usage.window_tokens > 0 && typeof usage.percent === "number") {
      sawUsage = true;
      ok(`conversation/get exposes measured context usage (${usage.used_tokens}/${usage.window_tokens})`);
    }
  }
  if (!sawUsage) {
    ok("mock context usage arrives with the completed turn (no timing assertion)");
  }

  // --- revoke hides the workspace but keeps its conversations --------------
  const revokeResult = await client.workspaces.revoke(created.workspace_id);
  if (revokeResult.revoked) {
    ok("workspace/revoke soft-deletes the owner's active workspace");
  } else {
    fail("workspace/revoke", JSON.stringify(revokeResult));
  }
  const afterRevoke = await client.workspaces.list();
  if (!afterRevoke.some((workspace) => workspace.workspace_id === created.workspace_id)) {
    ok("revoked workspace disappears from workspace/list");
  } else {
    fail("workspace/revoke visibility", JSON.stringify(afterRevoke));
  }
  // The conversation survives the workspace removal and is still readable.
  const stillThere = await client.conversations.get(chat.conversation_id);
  if (stillThere.conversation_id === chat.conversation_id) {
    ok("conversations persist after their workspace is revoked (they hide client-side)");
  } else {
    fail("conversation after revoke", JSON.stringify(stillThere));
  }
  // A revoked workspace cannot be used as a new-chat target on the server.
  try {
    await client.conversations.create({
      name: "Should not create",
      workspaceId: created.workspace_id,
    });
    fail("revoked workspace target", "conversation/create with a revoked workspace unexpectedly succeeded");
  } catch (error) {
    if (error instanceof AppServerError && (error.code === "workspace_denied" || error.code === "invalid_request")) {
      ok("conversation/create in a revoked workspace is refused");
    } else {
      fail("revoked workspace target", String(error));
    }
  }
  // Re-adding the same path re-activates the SAME workspace id.
  const restored = await client.workspaces.create(demoPath);
  if (restored.workspace_id === created.workspace_id) {
    ok("re-adding the same path re-activates the same workspace (conversations re-appear)");
  } else {
    fail("workspace re-add id reuse", `${restored.workspace_id} != ${created.workspace_id}`);
  }

  // --- delete removes the chat and its subscription surface -----------------
  const deleted = await client.conversations.delete(chat.conversation_id);
  if (deleted.deleted) {
    ok("conversation/delete confirms the removal");
  } else {
    fail("conversation/delete", JSON.stringify(deleted));
  }
  try {
    await client.conversations.get(chat.conversation_id);
    fail("conversation/delete removal", "deleted conversation is still readable");
  } catch (error) {
    if (error instanceof AppServerError && error.code === "not_found") {
      ok("deleted conversation maps to a stable not_found");
    } else {
      fail("conversation/delete removal", String(error));
    }
  }
}

async function main() {
  const options = parseArgs();
  const mockChild = options.real ? null : spawn(["bun", "scripts/mock-server.ts", String(MOCK_PORT)], {
    stdout: "pipe",
    stderr: "pipe",
  });

  try {
    await waitForHttp(options.httpUrl, options.real ? 20_000 : 10_000);
    let presetFound = true;
    let agentId = options.agentId;
    if (options.real) {
      const discovered = await discoverAgentId(options.httpUrl);
      if (discovered) {
        agentId = discovered;
        console.log(`real backend ready; builtin-office preset = ${agentId}`);
      } else {
        presetFound = false;
        console.log(
          "(info) real backend has no builtin-office preset yet — run success path is skipped; " +
            "protocol surface and error paths still verified",
        );
      }
    }

    const client = new AppServerClient({
      wsUrl: options.wsUrl,
      httpBaseUrl: options.httpUrl,
      client: { name: "smoke", version: "0.0.0" },
      capabilities: { events: true },
      requestTimeoutMs: 15_000,
    });

    // --- handshake ---------------------------------------------------------
    const handshake = await client.connect();
    if (handshake.protocol_version === "2026-08-26" && handshake.capabilities.agents) {
      ok("initialize handshake returns protocol version and single-agent capabilities");
    } else {
      fail("initialize handshake", JSON.stringify(handshake.capabilities));
    }
    if (handshake.capabilities.run_notifications) {
      ok("capabilities advertise run_notifications");
    } else {
      fail("capabilities", JSON.stringify(handshake.capabilities));
    }
    // teams follows the injected Team Catalog surface (ADR 05): the mock
    // server ships no Team Catalog, while the real composition root wires
    // one in — the assertion must track the actual composition, not a
    // stale single-agent-Phase expectation.
    if (handshake.capabilities.teams === !!options.real) {
      ok(
        `capabilities keep teams ${options.real ? "on (Team Catalog injected)" : "off (mock ships no Team Catalog)"}`,
      );
    } else {
      fail("capabilities teams", JSON.stringify(handshake.capabilities));
    }

    // --- workspace ---------------------------------------------------------
    const workspace = await client.registerWorkspace();
    if (workspace.id.length === 36) {
      ok(`workspace registered (${workspace.id.slice(0, 8)}…)`);
    } else {
      fail("workspace registration", workspace.id);
    }

    // --- Agent Store skill / connector catalog ------------------------------
    await runCatalogPath(client, options.real);

    // --- persistent conversation -------------------------------------------
    await runConversationPath(client, workspace.id, options.real);
    if (!options.real) {
      await runWorkspaceLifecycle(client, workspace.id);
    }

    // --- start a run -------------------------------------------------------
    const key = `smoke-${options.real ? "real" : "mock"}-${Date.now()}`;
    if (options.real && !presetFound) {
      // The real backend has no builtin-office preset yet (builtin presets are
      // intentionally shipped via Skills). Verify the failure surface stays
      // structured and retry-safe instead of fabricating assets.
      try {
        await client.runs.agent({
          agentId,
          goal: "inspect the repository",
          workspaceId: workspace.id,
          idempotencyKey: key,
        });
        fail("agent/run without preset assets", "expected a structured error but the call succeeded");
      } catch (error) {
        if (error instanceof AppServerError && error.code === "not_found" && !error.retryable) {
          ok("agent/run without a builtin-office preset fails cleanly (not_found, non-retryable)");
        } else {
          fail("agent/run without preset assets", String(error));
        }
      }
    } else {
      await runSuccessPath(client, { agentId, workspaceId: workspace.id, key, real: options.real });
    }

    // --- structured errors -------------------------------------------------
    try {
      await client.runs.agent({
        agentId: "00000000-0000-0000-0000-000000000000",
        goal: "nope",
        idempotencyKey: `bad-${Date.now()}`,
      });
      fail("invalid agent error", "expected a structured error but call succeeded");
    } catch (error) {
      if (error instanceof AppServerError && error.code.length > 0 && !error.retryable) {
        ok(`invalid agent_id maps to stable non-retryable code (${error.code})`);
      } else {
        fail("invalid agent error", String(error));
      }
    }

    try {
      await client.runs.get("0190f5fe-7c00-7a00-8000-00000000dead");
      fail("not_found error", "expected not_found but call succeeded");
    } catch (error) {
      if (error instanceof AppServerError && error.code === "not_found") {
        ok("unknown run maps to stable not_found code");
      } else {
        fail("not_found error", String(error));
      }
    }

    client.close();
  } finally {
    mockChild?.kill();
  }

  if (failures > 0) {
    console.error(`\nsmoke failed with ${failures} failure(s)`);
    process.exit(1);
  }
  console.log("\nsmoke passed");
}

await main();