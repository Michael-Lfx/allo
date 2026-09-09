/** WP-3 P0-C/D OAuth 运行时证据（TC-OAUTH-001/002/004），协议面全走 SDK 公共面。
 *
 * 本地 mock OAuth + MCP 平台（Bun.serve，127.0.0.1 回环）分四个资源场景：
 *   /good/mcp      完整 PKCE loopback + 可用的 MCP echo 工具  → TC-OAUTH-001
 *   /forbidden/mcp 授权成功但 MCP 资源恒 403                  → TC-OAUTH-004（403 边界）
 *   /mismatch/mcp  资源元数据指向不发布 RFC 8414 元数据的 AS   → TC-OAUTH-004（发现失败，无 token 请求）
 *   /timeout/mcp   authorize 不重定向 → 回环回调永不抵达      → TC-OAUTH-004（回调超时）
 *
 * 注意：宿主 login 的浏览器步由 `open::that` 触发 —— 本机默认浏览器会短暂打开
 * 3 次（good/forbidden 自动 302 回环回调；timeout 停在 mock 提示页），属预期。
 * 用法：AGENT_STORE_BIN=.../agent-store.exe bun scripts/sdk-live-oauth.ts
 */
import { launchClient } from "@agent-store/sdk";
import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

let failures = 0;
function check(name: string, ok: boolean, detail?: unknown): void {
  const suffix = detail === undefined ? "" : ` :: ${JSON.stringify(detail).slice(0, 300)}`;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${suffix}`);
  if (!ok) failures += 1;
}
const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

const encoder = new TextEncoder();
async function sha256B64url(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(input));
  return btoa(String.fromCharCode(...new Uint8Array(digest)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

// ---------------------------------------------------------------------------
// Mock OAuth + MCP platform
// ---------------------------------------------------------------------------
const SCENARIOS = ["good", "forbidden", "timeout", "mismatch"] as const;
type Scenario = (typeof SCENARIOS)[number];
interface Counters {
  register: number;
  authorize: number;
  pkceS256: number;
  tokenAuthCode: number;
  tokenRefresh: number;
  pkceFail: number;
  forbidden: number;
  issued: string[];
}
const counters: Record<Scenario, Counters> = {
  good: { register: 0, authorize: 0, pkceS256: 0, tokenAuthCode: 0, tokenRefresh: 0, pkceFail: 0, forbidden: 0, issued: [] },
  forbidden: { register: 0, authorize: 0, pkceS256: 0, tokenAuthCode: 0, tokenRefresh: 0, pkceFail: 0, forbidden: 0, issued: [] },
  timeout: { register: 0, authorize: 0, pkceS256: 0, tokenAuthCode: 0, tokenRefresh: 0, pkceFail: 0, forbidden: 0, issued: [] },
  mismatch: { register: 0, authorize: 0, pkceS256: 0, tokenAuthCode: 0, tokenRefresh: 0, pkceFail: 0, forbidden: 0, issued: [] },
};
const codes = new Map<string, string>(); // authorization code -> PKCE challenge
let codeSeq = 0;
// 声明在 mock 之前：handler 在运行时才被调用，届时已赋值。
let origin = "";

const json = (obj: unknown, status = 200): Response =>
  new Response(JSON.stringify(obj), { status, headers: { "content-type": "application/json" } });
const jsonRpc = (id: unknown, result: unknown, extraHeaders?: Record<string, string>): Response =>
  new Response(JSON.stringify({ jsonrpc: "2.0", id, result }), {
    status: 200,
    headers: { "content-type": "application/json", ...extraHeaders },
  });

const mock = Bun.serve({
  port: 0,
  async fetch(req: Request): Promise<Response> {
    const url = new URL(req.url);
    const path = url.pathname;

    // --- MCP protected resource -------------------------------------------------
    const resource = path.match(/^\/(good|forbidden|timeout|mismatch)\/mcp$/)?.[1] as Scenario | undefined;
    if (resource) {
      const c = counters[resource];
      if (req.method === "GET") {
        return new Response(null, {
          status: 401,
          headers: {
            "WWW-Authenticate": `Bearer error="invalid_request", resource_metadata="${origin}/.well-known/oauth-protected-resource/${resource}/mcp/"`,
          },
        });
      }
      if (resource === "forbidden") {
        c.forbidden += 1;
        return new Response("forbidden", { status: 403 });
      }
      const auth = req.headers.get("authorization");
      if (auth !== `Bearer oa-access-${resource}`) {
        return new Response(null, {
          status: 401,
          headers: { "WWW-Authenticate": `Bearer error="invalid_token"` },
        });
      }
      const body = await req.json().catch(() => null);
      if (body?.method === "initialize") {
        return jsonRpc(body.id, {
          protocolVersion: "2025-11-25",
          capabilities: { tools: {} },
          serverInfo: { name: `oauth-${resource}-mock`, version: "1.0.0" },
        }, { "mcp-session-id": `sess-${resource}` });
      }
      if (body?.method === "tools/list") {
        return jsonRpc(body.id, {
          tools: [{
            name: "echo",
            description: "Echo text back",
            inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] },
          }],
        });
      }
      return jsonRpc(body?.id ?? null, {});
    }

    // --- RFC 9728 protected-resource metadata ------------------------------------
    const prm = path.match(/^\/\.well-known\/oauth-protected-resource\/(good|forbidden|timeout|mismatch)\/mcp\/$/)?.[1] as Scenario | undefined;
    if (prm) {
      const authServer = prm === "mismatch" ? `${origin}/dead-oauth` : `${origin}/oauth-${prm}`;
      return json({
        resource: `${origin}/${prm}/mcp`,
        authorization_servers: [authServer],
        bearer_methods_supported: ["header"],
      });
    }

    // --- dead authorization server (mismatch boundary: no RFC 8414 metadata) -----
    if (path === "/dead-oauth/.well-known/oauth-authorization-server") {
      return new Response("not found", { status: 404 });
    }

    // --- RFC 8414 authorization server metadata ----------------------------------
    const meta = path.match(/^\/oauth-(good|forbidden|timeout)\/\.well-known\/oauth-authorization-server$/)?.[1] as "good" | "forbidden" | "timeout" | undefined;
    if (meta) {
      return json({
        issuer: `${origin}/oauth-${meta}`,
        authorization_endpoint: `${origin}/oauth-${meta}/authorize`,
        token_endpoint: `${origin}/oauth-${meta}/token`,
        registration_endpoint: `${origin}/oauth-${meta}/register`,
        code_challenge_methods_supported: ["S256"],
        scopes_supported: ["mcp"],
      });
    }

    // --- RFC 7591 dynamic client registration -------------------------------------
    const reg = path.match(/^\/oauth-(good|forbidden|timeout)\/register$/)?.[1] as "good" | "forbidden" | "timeout" | undefined;
    if (reg && req.method === "POST") {
      counters[reg].register += 1;
      const body = await req.json().catch(() => ({}));
      return json({
        client_id: `dyn-${reg}`,
        client_secret: null,
        redirect_uris: body.redirect_uris ?? [],
        grant_types: ["authorization_code", "refresh_token"],
        token_endpoint_auth_method: "none",
      }, 201);
    }

    // --- authorize (PKCE + state echo) ---------------------------------------------
    const au = path.match(/^\/oauth-(good|forbidden|timeout)\/authorize$/)?.[1] as "good" | "forbidden" | "timeout" | undefined;
    if (au) {
      const c = counters[au];
      c.authorize += 1;
      const challenge = url.searchParams.get("code_challenge") ?? "";
      if (challenge && url.searchParams.get("code_challenge_method") === "S256") c.pkceS256 += 1;
      const redirectUri = url.searchParams.get("redirect_uri") ?? `${origin}/callback`;
      const state = url.searchParams.get("state") ?? "";
      if (au === "timeout") {
        // 回调超时边界：authorize 永不重定向
        return new Response(
          "<html><body><h1>mock authorize</h1><p>no redirect (callback timeout case)</p></body></html>",
          { status: 200, headers: { "content-type": "text/html" } },
        );
      }
      const code = `oa-code-${au}-${++codeSeq}`;
      codes.set(code, challenge);
      return new Response(null, {
        status: 302,
        headers: { Location: `${redirectUri}?code=${encodeURIComponent(code)}&state=${encodeURIComponent(state)}` },
      });
    }

    // --- token (authorization_code with PKCE verification + refresh) ---------------
    const tk = path.match(/^\/oauth-(good|forbidden|timeout)\/token$/)?.[1] as "good" | "forbidden" | "timeout" | undefined;
    if (tk && req.method === "POST") {
      const c = counters[tk];
      const form = new URLSearchParams(await req.text());
      if (form.get("grant_type") === "refresh_token") {
        c.tokenRefresh += 1;
        const token = `oa-refresh-${tk}`;
        c.issued.push(token);
        return json({ access_token: token, token_type: "bearer", expires_in: 3600 });
      }
      const code = form.get("code") ?? "";
      const stored = codes.get(code);
      if (!stored || (await sha256B64url(form.get("code_verifier") ?? "")) !== stored) {
        c.pkceFail += 1;
        return json({ error: "invalid_grant", error_description: "PKCE verifier mismatch" }, 400);
      }
      codes.delete(code);
      c.tokenAuthCode += 1;
      const token = `oa-access-${tk}`;
      c.issued.push(token);
      return json({ access_token: token, token_type: "bearer", expires_in: 3600, refresh_token: `oa-refresh-${tk}` });
    }

    return new Response("not found", { status: 404 });
  },
});
origin = `http://127.0.0.1:${mock.port}`;
console.log(`MOCK OAuth+MCP ${origin}`);

// ---------------------------------------------------------------------------
// Host admin HTTP (not App Server protocol)
// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Main flow
// ---------------------------------------------------------------------------
const launched = await launchClient({
  requestTimeoutMs: 60_000,
  ...(process.env.OAUTH_KEEP_DATA === "1" ? { dataDir: join(tmpdir(), `agent-store-oauth-${Date.now()}`) } : {}),
  client: { name: "sdk-live-oauth", version: "1" },
});
const { server, client } = launched;
const base = `http://${server.readiness.host}:${server.readiness.port}`;
console.log(`LISTENING ${base} data=${server.dataDir}`);

try {
  // ---- 市场夹具：4 个 remote-mcp 连接器指向各场景资源 ----
  const marketRoot = await mkdtemp(join(tmpdir(), "oauth-market-"));
  const marketDir = join(marketRoot, "oauth-market");
  await mkdir(join(marketDir, ".codebuddy-connector"), { recursive: true });
  for (const name of SCENARIOS) {
    await mkdir(join(marketDir, "connectors", `oauth-${name}`), { recursive: true });
  }
  await writeFile(
    join(marketDir, ".codebuddy-connector/connectors.json"),
    JSON.stringify({
      name: "oauth-connectors",
      connectors: SCENARIOS.map((name) => ({
        id: `oauth-${name}`,
        name: `oauth-${name}`,
        version: "1.0.0",
        description: `OAuth scenario: ${name}`,
        type: "mcp",
      })),
    }),
  );
  for (const name of SCENARIOS) {
    await writeFile(
      join(marketDir, "connectors", `oauth-${name}`, "mcp.json"),
      JSON.stringify({
        mcpServers: { [`oauth-${name}`]: { type: "http", url: `${origin}/${name}/mcp` } },
      }),
    );
  }

  const market = await client.addMarketplace({ source_kind: "directory", source: marketDir });
  for (const name of SCENARIOS) {
    const install = await client.installStoreEntry(market.marketplace_id, `oauth-${name}`);
    check(`install.${name}`, install.installed_count > 0, install.installed_count);
  }
  const list = await client.connectors.list();
  const ids: Record<string, string> = {};
  for (const name of SCENARIOS) {
    const found = list.find((entry) => entry.name === `oauth-${name}`);
    ids[name] = found?.id ?? "";
  }
  check("catalog.4-registered", SCENARIOS.every((name) => Boolean(ids[name])), Object.fromEntries(SCENARIOS.map((name) => [name, ids[name]])));
  for (const name of SCENARIOS) {
    await adminPost(base, `/api/mcp/servers/${ids[name]}/toggle`, {});
  }

  // 预启动 timeout 流程已移除：login 串行门闩（并发 PKCE 会互相覆盖共享
  // pending 槽导致 CSRF mismatch），timeout 场景放到最后顺序执行。

  // ============================ TC-OAUTH-001 标准 PKCE Loopback ==============
  const before = await client.connectors.status(ids.good);
  check("OA-001.pre.status", before.status === "authorization_required" || before.status === "installed", before.status);
  const preAuth = await client.connectors.authStatus(ids.good);
  check("OA-001.pre.unauthenticated", preAuth.state === "not_authenticated", preAuth);

  const start = await client.connectors.authStart(ids.good);
  check("OA-001.auth-start-ack", start.state === "started", start);

  let authed: { state: string } | null = null;
  for (let i = 0; i < 100; i += 1) {
    authed = await client.connectors.authStatus(ids.good);
    if (authed.state === "authenticated") break;
    await sleep(300);
  }
  check("OA-001.authenticated", authed?.state === "authenticated", authed);

  const cGood = counters.good;
  check("OA-001.rfc7591-registration", cGood.register >= 1, cGood.register);
  check("OA-001.pkce-s256-challenge", cGood.pkceS256 >= 1, { authorize: cGood.authorize, pkceS256: cGood.pkceS256 });
  check("OA-001.token-exchange-authcode", cGood.tokenAuthCode === 1, cGood.tokenAuthCode);
  check("OA-001.pkce-verifier-validated", cGood.pkceFail === 0, cGood.pkceFail);

  // probe 证明请求时 Bearer 注入真实生效
  const probe = await client.connectors.test(ids.good);
  const tools = (probe.tools ?? []).map((tool) => tool.name);
  check("OA-001.probe-with-injected-token", probe.success && tools.includes("echo"), {
    success: probe.success,
    tools,
    error: probe.error,
  });
  const afterStatus = await client.connectors.status(ids.good);
  check("OA-001.post.connected", afterStatus.status === "connected", afterStatus.status);

  // ============================ TC-OAUTH-002 凭据隔离 ========================
  const surfaces: Record<string, string> = {
    "auth/status": JSON.stringify(authed),
    "connector/get": JSON.stringify(await client.connectors.get(ids.good)),
    "connector/status": JSON.stringify(afterStatus),
    "connector/list": JSON.stringify(await client.connectors.list()),
    "connector/test": JSON.stringify(probe),
  };
  const secrets = ["oa-access-good", "oa-refresh-good", ...cGood.issued];
  const leaked = Object.entries(surfaces).filter(([, body]) => secrets.some((secret) => body.includes(secret)));
  check("OA-002.no-token-in-public-surfaces", leaked.length === 0, leaked.map(([key]) => key));
  check("OA-002.issued-tokens-scanned", secrets.length > 0, secrets.length);

  // ============================ TC-OAUTH-004 错误边界 ========================
  // --- Issuer/Resource 不匹配：PRM 指向不发布 RFC 8414 元数据的 AS ---
  await client.connectors.authStart(ids.mismatch);
  await sleep(2500);
  const mmStatus = await client.connectors.authStatus(ids.mismatch);
  check("OA-004-mismatch.never-authenticated", mmStatus.state === "not_authenticated", mmStatus);
  check(
    "OA-004-mismatch.no-token-request",
    counters.mismatch.tokenAuthCode === 0 && counters.mismatch.tokenRefresh === 0,
    { authCode: counters.mismatch.tokenAuthCode, refresh: counters.mismatch.tokenRefresh },
  );
  check("OA-004-mismatch.no-authorize", counters.mismatch.authorize === 0, counters.mismatch.authorize);
  const mmProbe = await client.connectors.test(ids.mismatch).catch((error) => ({ success: false, error: String(error) }));
  check("OA-004-mismatch.probe-fails", !mmProbe.success, mmProbe);

  // --- 403：授权成功但资源恒 403 → 明确错误，不盲目刷新 ---
  await client.connectors.authStart(ids.forbidden);
  let forbAuthed: { state: string } | null = null;
  for (let i = 0; i < 100; i += 1) {
    forbAuthed = await client.connectors.authStatus(ids.forbidden);
    if (forbAuthed.state === "authenticated") break;
    await sleep(300);
  }
  check("OA-004-forbidden.authenticated", forbAuthed?.state === "authenticated", forbAuthed);
  const forbProbe = await client.connectors.test(ids.forbidden).catch((error) => ({ success: false, error: String(error) }));
  check(
    "OA-004-forbidden.403-clear-error",
    !forbProbe.success && String(forbProbe.error).includes("403"),
    { success: forbProbe.success, error: forbProbe.error },
  );
  check("OA-004-forbidden.no-blind-refresh", counters.forbidden.tokenRefresh === 0, counters.forbidden.tokenRefresh);
  const forbAfter = await client.connectors.authStatus(ids.forbidden);
  check("OA-004-forbidden.token-intact", forbAfter.state === "authenticated", forbAfter);

  // --- Callback 超时：authorize 不重定向 → 120s 回调超时 ---
  await client.connectors.authStart(ids.timeout);
  check("OA-004-timeout.pre.unauthenticated", (await client.connectors.authStatus(ids.timeout)).state === "not_authenticated");
  await sleep(125_000);
  const toStatus = await client.connectors.authStatus(ids.timeout);
  check("OA-004-timeout.not-authenticated", toStatus.state === "not_authenticated", toStatus);
  check(
    "OA-004-timeout.no-token-request",
    counters.timeout.tokenAuthCode === 0 && counters.timeout.tokenRefresh === 0,
    { authCode: counters.timeout.tokenAuthCode, refresh: counters.timeout.tokenRefresh },
  );
  const toProbe = await client.connectors.test(ids.timeout).catch((error) => ({ success: false, error: String(error) }));
  check("OA-004-timeout.probe-fails", !toProbe.success, toProbe);
} catch (error) {
  console.error("FAIL:", String(error).slice(0, 600));
  failures += 1;
} finally {
  console.log(`\nRESULT ${failures === 0 ? "PASS" : `FAIL(${failures})`} data=${server.dataDir}`);
  await server.close();
  mock.stop();
  process.exit(failures === 0 ? 0 : 1);
}
