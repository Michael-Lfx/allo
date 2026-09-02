/**
 * Mock Agent Store App Server for offline Web UI development.
 *
 * Serves both faces of the V1 protocol on one port:
 *   - HTTP  : POST /api/app-server/initialize (returns x-app-server-connection-id)
 *             POST /api/app-server/workspaces
 *   - WS    : /api/app-server/ws — JSON-RPC 2.0 (initialize/initialized/ping,
 *             agent/run, run/get|result|events|cancel, run/subscribe|unsubscribe)
 *
 * Run with: bun scripts/mock-server.ts [port]   (default 17860)
 * Point the Web UI at ws://127.0.0.1:17860/api/app-server/ws and
 * http://127.0.0.1:17860/api/app-server.
 */

import type { ServerWebSocket } from "bun";

const PORT = Number(process.argv[2] ?? 17860);
const PROTOCOL_VERSION = "2026-08-26";
const AGENT_ID = "0190f5fe-7c00-7a00-8000-000000000004";

interface MockRun {
  runId: string;
  status: string;
  version: number;
  summary: string | null;
  outputFiles: string[];
  events: Array<{ run_id: string; sequence: number; event_type: string; payload: Record<string, unknown> }>;
}
interface MockConversation {
  id: string;
  name: string;
  model: { provider_id: string; model: string };
  status: string;
  created_at: number;
  modified_at: number;
  messages: Array<Record<string, unknown>>;
  processing: boolean;
  sequence: number;
  reasoning_effort?: string;
  workspace_id?: string;
  context_usage?: unknown;
}

interface MockWorkspace {
  workspace_id: string;
  root_path: string;
  created_at: number;
  updated_at: number;
}

const runs = new Map<string, MockRun>();
const conversations = new Map<string, MockConversation>();
const workspaces = new Map<string, MockWorkspace>();
const idempotent = new Map<string, Record<string, unknown>>();
const conversationIdempotent = new Map<string, Record<string, unknown>>();
const subscriptions = new Map<ServerWebSocket, { runs: Set<string>; conversations: Set<string> }>();
let sequenceCounter = 100;

function workspaceName(rootPath: string): string {
  const normalized = rootPath.replace(/[\\/]+$/, "");
  const segments = normalized.split(/[\\/]/);
  const last = segments[segments.length - 1];
  return last && last.length > 0 ? last : rootPath;
}

function workspaceView(workspace: MockWorkspace) {
  return {
    workspace_id: workspace.workspace_id,
    name: workspaceName(workspace.root_path),
    canonical_path: workspace.root_path,
    created_at: workspace.created_at,
    updated_at: workspace.updated_at,
  };
}

function ensureWorkspace(path: string): MockWorkspace {
  for (const workspace of workspaces.values()) {
    if (workspace.root_path === path) {
      workspace.updated_at = Date.now();
      // Re-adding the same path re-activates a previously revoked workspace.
      delete (workspace as MockWorkspace & { revoked?: boolean }).revoked;
      return workspace;
    }
  }
  const workspace: MockWorkspace = {
    workspace_id: uuidv7(),
    root_path: path,
    created_at: Date.now(),
    updated_at: Date.now(),
  };
  workspaces.set(workspace.workspace_id, workspace);
  return workspace;
}

function uuidv7(): string {
  const ts = BigInt(Date.now());
  const tsHex = ts.toString(16).padStart(12, "0");
  const random = crypto.getRandomValues(new Uint8Array(10));
  random[0] = (random[0] & 0x0f) | 0x70; // version 7
  random[2] = (random[2] & 0x3f) | 0x80; // variant
  const body = Array.from(random)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  const raw = tsHex + body;
  return `${raw.slice(0, 8)}-${raw.slice(8, 12)}-${raw.slice(12, 16)}-${raw.slice(16, 20)}-${raw.slice(20)}`;
}

function capabilities() {
  return {
    agents: true,
    teams: false,
    team_runtime: false,
    skills: true,
    connectors: true,
    run_notifications: true,
    approvals: false,
    artifacts: false,
    oauth: true,
  };
}

// ---------------------------------------------------------------------------
// Agent Store catalog mock data (Skills + Connectors)
// ---------------------------------------------------------------------------

interface MockSkill {
  id: string;
  name: string;
  description: string;
  version: string;
  source: string;
  compatibility_status: string;
  enabled: boolean;
  required_connectors: string[];
  mode: string;
  invocation_policy: string;
  instructions_summary: string;
}

interface MockConnector {
  id: string;
  name: string;
  description: string | null;
  kind: string;
  transport_summary: string;
  auth_mode: "none" | "oauth";
  enabled: boolean;
  tools: Array<{ name: string; description: string | null }>;
  /** Last probe outcome; `false` must never surface as `connected`. */
  probe_success: boolean;
}

const catalogSkills: MockSkill[] = [
  {
    id: "pdf",
    name: "pdf",
    description: "解析 PDF 文档并提取结构化内容。",
    version: "builtin",
    source: "builtin",
    compatibility_status: "compatible",
    enabled: true,
    required_connectors: [],
    mode: "store-agent",
    invocation_policy: "model-auto",
    instructions_summary: "按需解析本地 PDF 文件：提取文本、表格与元数据，输出 Markdown。",
  },
  {
    id: "cron",
    name: "cron",
    description: "创建和管理定时任务。",
    version: "builtin",
    source: "builtin",
    compatibility_status: "compatible",
    enabled: true,
    required_connectors: [],
    mode: "client-instructions",
    invocation_policy: "/invoke",
    instructions_summary: "按用户意图创建、查询、暂停或删除定时任务。",
  },
  {
    id: "release-notes",
    name: "release-notes",
    description: "生成版本发布说明（自定义技能示例）。",
    version: "custom",
    source: "custom",
    compatibility_status: "compatible-with-adapter",
    enabled: true,
    required_connectors: [],
    mode: "store-workflow",
    invocation_policy: "/invoke",
    instructions_summary: "根据提交记录与里程碑生成中英文发布说明。",
  },
];

const CONNECTOR_IDS = {
  playwright: "0190f5fe-7c00-7a00-8000-000000000101",
  github: "0190f5fe-7c00-7a00-8000-000000000102",
  failing: "0190f5fe-7c00-7a00-8000-000000000103",
};

const catalogConnectors: MockConnector[] = [
  {
    id: CONNECTOR_IDS.playwright,
    name: "playwright",
    description: "浏览器测试自动化（本地 stdio）。",
    kind: "stdio-mcp",
    transport_summary: "npx @playwright/mcp",
    auth_mode: "none",
    enabled: true,
    tools: [
      { name: "browser_navigate", description: "打开页面" },
      { name: "browser_click", description: "点击元素" },
      { name: "browser_snapshot", description: "获取页面快照" },
    ],
    probe_success: true,
  },
  {
    id: CONNECTOR_IDS.github,
    name: "github",
    description: "GitHub API（远程 MCP + OAuth）。",
    kind: "remote-mcp",
    transport_summary: "https://api.githubcopilot.com/mcp/",
    auth_mode: "oauth",
    enabled: true,
    tools: [
      { name: "github_list_repos", description: "列出仓库" },
      { name: "github_create_issue", description: "创建 Issue" },
    ],
    probe_success: false,
  },
  {
    id: CONNECTOR_IDS.failing,
    name: "flaky-bridge",
    description: "持续探测失败的连接器示例。",
    kind: "stdio-mcp",
    transport_summary: "npx flaky-bridge",
    auth_mode: "none",
    enabled: true,
    tools: [],
    probe_success: false,
  },
];

/** id → OAuth authenticated state (mock). */
const connectorAuth = new Map<string, boolean>();

function skillSummary(skill: MockSkill) {
  const { mode: _mode, invocation_policy: _policy, instructions_summary: _summary, ...summary } = skill;
  return summary;
}

function connectorSummary(connector: MockConnector) {
  return {
    id: connector.id,
    name: connector.name,
    description: connector.description,
    kind: connector.kind,
    transport_summary: connector.transport_summary,
    auth_mode: connector.auth_mode,
    enabled: connector.enabled,
    status: mockConnectorStatus(connector),
  };
}

function mockConnectorStatus(connector: MockConnector): string {
  if (!connector.enabled) return "installed";
  if (!connector.probe_success) return "error";
  if (connector.auth_mode === "oauth" && !connectorAuth.get(connector.id)) return "authorization_required";
  return "connected";
}

function connectorDetail(connector: MockConnector) {
  return {
    ...connectorSummary(connector),
    tool_filter: `connector__${connector.name}__<tool>`,
    tools: connector.tools,
    auth_status: connector.auth_mode === "oauth"
      ? { state: connectorAuth.get(connector.id) ? "authenticated" : "not_authenticated", error: null }
      : null,
    source: "system",
    compatibility_status: "compatible",
  };
}

function connectorStatusView(connector: MockConnector) {
  return {
    connector_id: connector.id,
    status: mockConnectorStatus(connector),
    auth_status: connector.auth_mode === "oauth"
      ? { state: connectorAuth.get(connector.id) ? "authenticated" : "not_authenticated", error: null }
      : null,
    last_error: connector.probe_success ? null : "last connection test failed",
  };
}

function findConnector(connectorId: string): MockConnector | undefined {
  return catalogConnectors.find((connector) => connector.id === connectorId || connector.name === connectorId);
}

function receiptFor(run: MockRun) {
  return {
    run_id: run.runId,
    status: run.status,
    version: run.version,
    preset_revision: 1,
    content_digest: "sha256:mock",
  };
}

function viewFor(run: MockRun) {
  return {
    run_id: run.runId,
    status: run.status,
    version: run.version,
    summary: run.summary,
    output_files: run.outputFiles,
    preset_revision: 1,
    content_digest: "sha256:mock",
  };
}

function pushEvent(run: MockRun, eventType: string, payload: Record<string, unknown>, delayMs: number) {
  setTimeout(() => {
    const next = runs.get(run.runId);
    if (!next || next.status === "cancelled") {
      return;
    }
    const sequence = ++sequenceCounter;
    const event = { run_id: run.runId, sequence, event_type: eventType, payload };
    next.events.push(event);
    for (const [socket, set] of subscriptions) {
      if (set.runs.has(run.runId)) {
        socket.send(JSON.stringify({ jsonrpc: "2.0", method: "event", params: event }));
      }
    }
  }, delayMs);
}

function progressRun(runId: string) {
  const run = runs.get(runId);
  if (!run) {
    return;
  }
  pushEvent(run, "run.started", { status: "planning" }, 150);
  pushEvent(run, "run.status_changed", { status: "running" }, 600);
  pushEvent(run, "task.updated", { title: "Read the repository layout", status: "running" }, 1100);
  pushEvent(run, "task.updated", { title: "Inspect build and test commands", status: "running" }, 1700);
  pushEvent(run, "task.updated", { title: "Write the findings report", status: "completed" }, 2300);
  setTimeout(() => {
    const current = runs.get(runId);
    if (!current || current.status === "cancelled") {
      return;
    }
    current.status = "completed";
    current.version += 1;
    current.summary = "Repository inspected; no blockers found.";
    current.outputFiles = ["findings.md"];
    for (const [socket, set] of subscriptions) {
      if (set.runs.has(runId)) {
        socket.send(
          JSON.stringify({
            jsonrpc: "2.0",
            method: "event",
            params: {
              run_id: runId,
              sequence: ++sequenceCounter,
              event_type: "run.status_changed",
              payload: { status: "completed" },
            },
          }),
        );
      }
    }
  }, 2900);
}

function respond(socket: ServerWebSocket, id: unknown, result: unknown) {
  socket.send(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

function reject(socket: ServerWebSocket, id: unknown, code: string, message: string, retryable = false) {
  socket.send(
    JSON.stringify({
      jsonrpc: "2.0",
      id,
      error: { code, message, retryable, details: {} },
    }),
  );
}

function sendConversationEvent(conversation: MockConversation, eventType: string, payload: Record<string, unknown>) {
  const event = {
    conversation_id: conversation.id,
    sequence: ++conversation.sequence,
    event_type: eventType,
    payload,
  };
  conversation.modified_at = Date.now();
  for (const [socket, set] of subscriptions) {
    if (set.conversations.has(conversation.id)) {
      socket.send(JSON.stringify({ jsonrpc: "2.0", method: "conversation/event", params: event }));
    }
  }
}

function conversationView(conversation: MockConversation) {
  return {
    conversation_id: conversation.id,
    name: conversation.name,
    model: conversation.model,
    status: conversation.status,
    created_at: conversation.created_at,
    modified_at: conversation.modified_at,
    is_processing: conversation.processing,
    workspace_id: conversation.workspace_id,
    context_usage: conversation.context_usage ?? null,
  };
}

function conversationMessages(conversation: MockConversation) {
  return conversation.messages;
}

function runConversationTurn(conversation: MockConversation, userMessage: Record<string, unknown>) {
  conversation.processing = true;
  conversation.status = "running";
  const thinkingId = uuidv7();
  const thinkingSubject = "正在分析请求";
  const thinkingChunks = ["先确认用户的输入和上下文。", "然后组织一个清晰、简洁的答复。"];
  sendConversationEvent(conversation, "turn.status", { turn_id: userMessage.message_id, status: "running" });
  setTimeout(() => {
    sendConversationEvent(conversation, "message.thinking", {
      message_id: thinkingId,
      content: thinkingChunks[0],
      subject: thinkingSubject,
      status: "thinking",
      replace: false,
    });
  }, 150);
  setTimeout(() => {
    sendConversationEvent(conversation, "message.thinking", {
      message_id: thinkingId,
      content: thinkingChunks[1],
      status: "thinking",
      replace: false,
    });
  }, 330);
  setTimeout(() => {
    sendConversationEvent(conversation, "message.tips", {
      message_id: uuidv7(),
      content: "这是 mock 模式的演示提示，可点击展开查看详情。",
      tip_type: "success",
    });
  }, 240);
  setTimeout(() => {
    sendConversationEvent(conversation, "message.tool", {
      message_id: uuidv7(),
      name: "read_file",
      status: "completed",
    });
  }, 400);
  setTimeout(() => {
    const duration = 410;
    conversation.messages.push({
      message_id: thinkingId,
      conversation_id: conversation.id,
      role: "activity",
      content: JSON.stringify({ content: thinkingChunks.join(""), subject: thinkingSubject, status: "done", duration }),
      message_type: "thinking",
      status: "finish",
      created_at: Date.now(),
    });
    sendConversationEvent(conversation, "message.thinking", {
      message_id: thinkingId,
      content: "",
      status: "done",
      duration,
      replace: false,
    });
  }, 560);
  setTimeout(() => {
    const assistantId = uuidv7();
    const text = `收到：${String(userMessage.content ?? "")}\n\n这是 mock 模式的流式回复。连接真实后端后，回复将来自已配置的 Nomi 模型。`;
    conversation.messages.push({ message_id: assistantId, conversation_id: conversation.id, role: "assistant", content: text, message_type: "text", status: "finish", created_at: Date.now() });
    sendConversationEvent(conversation, "message.delta", { message_id: assistantId, content: text, replace: true });
    // Measured context occupancy (mock): after every completed turn the server
    // persists a fresh snapshot and pushes it to subscribers, mirroring the
    // real App Server `context.usage` projection.
    const updated_at = Date.now();
    conversation.context_usage = { used_tokens: 5321, window_tokens: 200000, percent: 2.7, updated_at, source: "measured" };
    sendConversationEvent(conversation, "context.usage", { context_usage: conversation.context_usage });
    conversation.processing = false;
    conversation.status = "finished";
    sendConversationEvent(conversation, "turn.status", { turn_id: userMessage.message_id, status: "completed" });
  }, 900);
}

function handleConversationMethod(socket: ServerWebSocket, id: unknown, method: string, params: Record<string, unknown> | undefined) {
  if (method === "conversation/model-options") {
    respond(socket, id, {
      default: { provider: "opencode", model: "mimo-v2.5-free" },
      providers: [
        {
          name: "opencode",
          models: [
            { name: "mimo-v2.5-free", display_name: "MiMo V2.5 Free", context_limit: 200000 },
            { name: "laguna-s-2.1-free", display_name: "Laguna S 2.1 Free", context_limit: 256000 },
          ],
        },
      ],
      reasoning_efforts: ["low", "medium", "high", "xhigh"],
    });
    return true;
  }
  if (method === "conversation/create") {
    const now = Date.now();
    let workspaceId: string | undefined;
    const workspaceRef = (params?.workspace as { id?: string } | undefined)?.id;
    if (workspaceRef) {
      const target = workspaces.get(workspaceRef);
      if (!target || (target as MockWorkspace & { revoked?: boolean }).revoked) {
        reject(socket, id, "workspace_denied", "workspace is revoked or not registered");
        return true;
      }
      workspaceId = workspaceRef;
    }
    const conversation: MockConversation = { id: uuidv7(), name: String(params?.name ?? "新对话"), model: (params?.model as { provider_id: string; model: string }) ?? { provider_id: "opencode", model: "mimo-v2.5-free" }, status: "pending", created_at: now, modified_at: now, messages: [], processing: false, sequence: 0, workspace_id: workspaceId };
    if (typeof params?.reasoning_effort === "string" && params.reasoning_effort) {
      conversation.reasoning_effort = params.reasoning_effort;
    }
    conversations.set(conversation.id, conversation);
    respond(socket, id, conversationView(conversation));
    return true;
  }
  if (method === "conversation/delete") {
    const conversationId = String(params?.conversation_id ?? "");
    if (!conversations.has(conversationId)) {
      reject(socket, id, "not_found", "conversation not found");
      return true;
    }
    conversations.delete(conversationId);
    for (const [, set] of subscriptions) set.conversations.delete(conversationId);
    respond(socket, id, { conversation_id: conversationId, deleted: true });
    return true;
  }
  if (method === "workspace/list") {
    const active = [...workspaces.values()].filter((workspace) => !(workspace as MockWorkspace & { revoked?: boolean }).revoked);
    respond(socket, id, active.sort((a, b) => b.updated_at - a.updated_at).map(workspaceView));
    return true;
  }
  if (method === "workspace/create") {
    const path = typeof params?.path === "string" ? params.path.trim() : "";
    if (!path) {
      reject(socket, id, "invalid_request", "workspace path must not be empty");
      return true;
    }
    if (!path.startsWith("/") && !/^[A-Za-z]:[\\/]/.test(path)) {
      reject(socket, id, "workspace_denied", "workspace path must be an absolute directory path");
      return true;
    }
    respond(socket, id, workspaceView(ensureWorkspace(path)));
    return true;
  }
  if (method === "workspace/revoke") {
    const workspaceId = String(params?.workspace_id ?? "");
    const workspace = workspaces.get(workspaceId);
    if (!workspace) {
      respond(socket, id, { workspace_id: workspaceId, revoked: false });
      return true;
    }
    // Soft delete: mark revoked and exclude from list_active (the mock keeps
    // conversations untouched so they hide until the workspace is re-added).
    workspace.updated_at = Date.now();
    (workspace as MockWorkspace & { revoked?: boolean }).revoked = true;
    respond(socket, id, { workspace_id: workspaceId, revoked: true });
    return true;
  }
  if (method === "conversation/list") {
    respond(socket, id, [...conversations.values()].sort((a, b) => b.modified_at - a.modified_at).map(conversationView));
    return true;
  }
  const conversationId = String(params?.conversation_id ?? "");
  const conversation = conversations.get(conversationId);
  if (!conversation && method.startsWith("conversation/")) { reject(socket, id, "not_found", "conversation not found"); return true; }
  if (method === "conversation/get" && conversation) respond(socket, id, conversationView(conversation));
  else if (method === "conversation/update" && conversation) {
    if (params?.name !== undefined) conversation.name = String(params.name);
    if (params?.model !== undefined) conversation.model = params.model as { provider_id: string; model: string };
    if (typeof params?.reasoning_effort === "string") {
      conversation.reasoning_effort = params.reasoning_effort;
    }
    conversation.modified_at = Date.now();
    respond(socket, id, conversationView(conversation));
  }
  else if (method === "conversation/messages" && conversation) respond(socket, id, conversationMessages(conversation));
  else if (method === "conversation/send" && conversation) {
    const key = String(params?.idempotency_key ?? "");
    const replay = conversationIdempotent.get(key);
    if (replay) respond(socket, id, replay);
    else {
      const message = { message_id: uuidv7(), conversation_id: conversation.id, role: "user", content: String(params?.content ?? ""), message_type: "text", status: "finish", created_at: Date.now() };
      conversation.messages.push(message);
      const receipt = { conversation_id: conversation.id, message_id: message.message_id, turn_id: message.message_id, accepted: true, replayed: false, completed: false };
      conversationIdempotent.set(key, receipt);
      sendConversationEvent(conversation, "message.created", { message_id: message.message_id, role: "user", content: message.content, created_at: message.created_at });
      runConversationTurn(conversation, message);
      respond(socket, id, receipt);
    }
  } else if (method === "conversation/cancel" && conversation) { conversation.processing = false; conversation.status = "finished"; respond(socket, id, conversationView(conversation)); }
  else if ((method === "conversation/subscribe" || method === "conversation/unsubscribe") && conversation) {
    const set = subscriptions.get(socket) ?? { runs: new Set<string>(), conversations: new Set<string>() };
    const subscribing = method === "conversation/subscribe";
    if (subscribing) set.conversations.add(conversation.id); else set.conversations.delete(conversation.id);
    subscriptions.set(socket, set);
    respond(socket, id, { conversation_id: conversation.id, subscribed: subscribing });
  } else return false;
  return true;
}

function handleWsMessage(socket: ServerWebSocket, raw: string) {
  let message: { id?: unknown; method?: string; params?: Record<string, unknown> };
  try {
    message = JSON.parse(raw);
  } catch {
    return;
  }
  const { id, method, params } = message;
  if (!method) {
    return;
  }
  if ((method.startsWith("conversation/") || method.startsWith("workspace/")) && handleConversationMethod(socket, id, method, params)) {
    return;
  }

  switch (method) {
    case "initialize":
      respond(socket, id, {
        protocol_version: PROTOCOL_VERSION,
        server: { name: "allo-agent-store (mock)", version: "0.1.0" },
        auth_context: {
          principal_id: "0190f5fe-7c00-7a00-8000-00000000000a",
          issuer: "local-agent-store",
          audience: "agent-store",
          scopes: ["catalog:read", "run:read", "run:write"],
        },
        capabilities: capabilities(),
        connection_id: uuidv7(),
      });
      return;
    case "initialized":
      respond(socket, id, { ok: true });
      return;
    case "ping":
      respond(socket, id, { ok: true });
      return;
    case "agent/run": {
      const agentId = String(params?.agent_id ?? "");
      if (agentId !== AGENT_ID) {
        reject(socket, id, "invalid_request", `unknown or unsupported agent_id: ${agentId}`);
        return;
      }
      const key = typeof params?.idempotency_key === "string" ? params.idempotency_key : null;
      if (key && idempotent.has(key)) {
        respond(socket, id, idempotent.get(key));
        return;
      }
      const run = {
        runId: uuidv7(),
        status: "planning",
        version: 1,
        summary: null,
        outputFiles: [] as string[],
        events: [],
      };
      runs.set(run.runId, run);
      if (key) {
        idempotent.set(key, receiptFor(run));
      }
      progressRun(run.runId);
      respond(socket, id, receiptFor(run));
      return;
    }
    case "run/get": {
      const run = runs.get(String(params?.run_id ?? ""));
      if (!run) {
        reject(socket, id, "not_found", "run not found");
        return;
      }
      respond(socket, id, viewFor(run));
      return;
    }
    case "run/result": {
      const run = runs.get(String(params?.run_id ?? ""));
      if (!run) {
        reject(socket, id, "not_found", "run not found");
        return;
      }
      if (run.status !== "completed" && run.status !== "failed" && run.status !== "cancelled" && run.status !== "completed_with_failures") {
        reject(socket, id, "conflict", "run result is only available after the run reaches a terminal state");
        return;
      }
      respond(socket, id, viewFor(run));
      return;
    }
    case "run/events": {
      const run = runs.get(String(params?.run_id ?? ""));
      if (!run) {
        reject(socket, id, "not_found", "run not found");
        return;
      }
      const after = Number(params?.after_sequence ?? 0);
      const limit = Number(params?.limit ?? 200);
      respond(
        socket,
        id,
        run.events
          .filter((event) => event.sequence > after)
          .slice(-limit),
      );
      return;
    }
    case "run/cancel": {
      const run = runs.get(String(params?.run_id ?? ""));
      if (!run) {
        reject(socket, id, "not_found", "run not found");
        return;
      }
      const expectedVersion = Number(params?.expected_version ?? 1);
      if (expectedVersion !== run.version) {
        reject(socket, id, "run_not_resumable", "stale Agent Execution version");
        return;
      }
      run.status = "cancelled";
      run.version += 1;
      for (const [socketOf, set] of subscriptions) {
        if (set.runs.has(run.runId)) {
          socketOf.send(
            JSON.stringify({
              jsonrpc: "2.0",
              method: "event",
              params: {
                run_id: run.runId,
                sequence: ++sequenceCounter,
                event_type: "run.status_changed",
                payload: { status: "cancelled", reason: "cancelled_by_caller" },
              },
            }),
          );
        }
      }
      respond(socket, id, viewFor(run));
      return;
    }
    case "run/subscribe": {
      const runId = String(params?.run_id ?? "");
      if (!runs.has(runId)) {
        reject(socket, id, "not_found", "run not found");
        return;
      }
      const set = subscriptions.get(socket) ?? { runs: new Set<string>(), conversations: new Set<string>() };
      set.runs.add(runId);
      subscriptions.set(socket, set);
      respond(socket, id, { run_id: runId, subscribed: true });
      return;
    }
    case "run/unsubscribe": {
      const runId = String(params?.run_id ?? "");
      subscriptions.get(socket)?.runs.delete(runId);
      respond(socket, id, { run_id: runId, subscribed: false });
      return;
    }
    case "skill/list":
      respond(socket, id, catalogSkills.map(skillSummary));
      return;
    case "skill/get": {
      const skill = catalogSkills.find((entry) => entry.id === String(params?.skill_id ?? ""));
      if (!skill) {
        reject(socket, id, "not_found", "skill not found");
        return;
      }
      respond(socket, id, skill);
      return;
    }
    case "connector/list":
      respond(socket, id, catalogConnectors.map(connectorSummary));
      return;
    case "connector/get": {
      const connector = findConnector(String(params?.connector_id ?? ""));
      if (!connector) {
        reject(socket, id, "not_found", "connector not found");
        return;
      }
      respond(socket, id, connectorDetail(connector));
      return;
    }
    case "connector/status": {
      const connector = findConnector(String(params?.connector_id ?? ""));
      if (!connector) {
        reject(socket, id, "not_found", "connector not found");
        return;
      }
      respond(socket, id, connectorStatusView(connector));
      return;
    }
    case "connector/test": {
      const connector = findConnector(String(params?.connector_id ?? ""));
      if (!connector) {
        reject(socket, id, "not_found", "connector not found");
        return;
      }
      if (connector.id === CONNECTOR_IDS.failing) {
        respond(socket, id, {
          connector_id: connector.id,
          success: false,
          tools: [],
          error: "connection refused",
          code: "MCP_CONNECTION_FAILED",
        });
        return;
      }
      connector.probe_success = true;
      respond(socket, id, {
        connector_id: connector.id,
        success: true,
        tools: connector.tools,
        error: null,
        code: null,
      });
      return;
    }
    case "connector/auth/start": {
      const connector = findConnector(String(params?.connector_id ?? ""));
      if (!connector) {
        reject(socket, id, "not_found", "connector not found");
        return;
      }
      if (connector.auth_mode !== "oauth") {
        reject(socket, id, "invalid_request", "OAuth is not supported for this connector");
        return;
      }
      // Async browser flow on the trusted host; clients poll auth/status.
      setTimeout(() => {
        connectorAuth.set(connector.id, true);
      }, 300);
      respond(socket, id, { connector_id: connector.id, state: "started", error: null });
      return;
    }
    case "connector/auth/status": {
      const connector = findConnector(String(params?.connector_id ?? ""));
      if (!connector) {
        reject(socket, id, "not_found", "connector not found");
        return;
      }
      respond(socket, id, {
        state: connector.auth_mode === "oauth" && connectorAuth.get(connector.id)
          ? "authenticated"
          : "not_authenticated",
        error: null,
      });
      return;
    }
    case "connector/auth/logout": {
      const connector = findConnector(String(params?.connector_id ?? ""));
      if (!connector) {
        reject(socket, id, "not_found", "connector not found");
        return;
      }
      connectorAuth.delete(connector.id);
      respond(socket, id, { connector_id: connector.id, logged_out: true });
      return;
    }
    default:
      reject(socket, id, "invalid_request", `unknown App Server method: ${method}`);
  }
}

const server = Bun.serve({
  port: PORT,
  fetch(request, server) {
    const url = new URL(request.url);
    if (url.pathname === "/api/app-server/ws") {
      if (server.upgrade(request)) {
        return undefined;
      }
      return new Response("upgrade failed", { status: 400 });
    }
    if (url.pathname === "/api/app-server/initialize" && request.method === "POST") {
      const connectionId = uuidv7();
      return new Response(
        JSON.stringify({
          protocol_version: PROTOCOL_VERSION,
          server: { name: "allo-agent-store (mock)", version: "0.1.0" },
          auth_context: {
            principal_id: "0190f5fe-7c00-7a00-8000-00000000000a",
            issuer: "local-agent-store",
            audience: "agent-store",
            scopes: ["catalog:read", "run:read", "run:write"],
          },
          capabilities: capabilities(),
          connection_id: connectionId,
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
            "x-app-server-connection-id": connectionId,
          },
        },
      );
    }
    if (url.pathname === "/api/app-server/initialized" && request.method === "POST") {
      if (!request.headers.get("x-app-server-connection-id")) {
        return new Response(
          JSON.stringify({ code: "invalid_request", message: "app-server connection id is required", retryable: false, details: {} }),
          { status: 400, headers: { "content-type": "application/json" } },
        );
      }
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    if (url.pathname === "/api/app-server/workspaces" && request.method === "POST") {
      const connectionId = request.headers.get("x-app-server-connection-id");
      if (!connectionId) {
        return new Response(
          JSON.stringify({ code: "invalid_request", message: "app-server connection id is required", retryable: false, details: {} }),
          { status: 400, headers: { "content-type": "application/json" } },
        );
      }
      const workspaceId = uuidv7();
      const rootPath = `${process.cwd().replace(/\\/g, "/")}/.mock-workspaces/${workspaceId}`;
      workspaces.set(workspaceId, { workspace_id: workspaceId, root_path: rootPath, created_at: Date.now(), updated_at: Date.now() });
      return new Response(JSON.stringify({ id: workspaceId }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    return new Response("not found", { status: 404 });
  },
  websocket: {
    open(socket) {
      subscriptions.set(socket, { runs: new Set(), conversations: new Set() });
    },
    message(socket, raw) {
      handleWsMessage(socket, String(raw));
    },
    close(socket) {
      subscriptions.delete(socket);
    },
  },
});

console.log(`Mock Agent Store App Server listening on http://127.0.0.1:${server.port}`);
console.log(`WS : ws://127.0.0.1:${server.port}/api/app-server/ws`);
console.log(`HTTP: http://127.0.0.1:${server.port}/api/app-server`);