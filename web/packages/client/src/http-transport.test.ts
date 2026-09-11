import { describe, expect, it } from "vitest";

import { AppServerError, TransportError } from "@flowy-agent-store/protocol";
import { HttpTransport, appServerErrorFromWire, httpRouteTable } from "./http-transport";

interface Call {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: unknown;
}

/** Scripted fetch: the handshake answers get a connection header, business calls the script. */
function fakeFetch(script: Array<{ status?: number; body?: unknown }> = []) {
  const calls: Call[] = [];
  let index = 0;
  const impl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const headers = (init?.headers ?? {}) as Record<string, string>;
    calls.push({
      method: init?.method ?? "GET",
      url: String(input),
      headers,
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    });
    const isHandshake = String(input).endsWith("/initialize") || String(input).endsWith("/initialized");
    if (String(input).endsWith("/initialize")) {
      return new Response(JSON.stringify({ protocol_version: "x" }), {
        status: 200,
        headers: { "x-app-server-connection-id": "conn-1" },
      });
    }
    if (String(input).endsWith("/initialized")) {
      return new Response(JSON.stringify({ ok: true }), { status: 200 });
    }
    const step = script[index++] ?? { status: 200, body: { ok: true } };
    return new Response(JSON.stringify(step.body ?? {}), {
      status: step.status ?? 200,
      headers: { "content-type": "application/json" },
    });
  }) as unknown as typeof fetch;
  return { calls, impl };
}

function transport(script: Array<{ status?: number; body?: unknown }> = []) {
  const fake = fakeFetch(script);
  const instance = new HttpTransport({
    baseUrl: "http://127.0.0.1:8787/api/app-server",
    fetch: fake.impl,
    client: { name: "test", version: "0" },
  });
  return { instance, calls: fake.calls };
}

const businessCalls = (calls: Call[]) =>
  calls.filter((call) => !call.url.endsWith("/initialize") && !call.url.endsWith("/initialized"));

describe("HttpTransport", () => {
  it("handshakes once per call, then issues the mapped business request", async () => {
    const { instance, calls } = transport();
    await instance.request("conversation/send", {
      conversation_id: "c1",
      content: "hi",
      idempotency_key: "k1",
    });

    const all = calls.map((call) => `${call.method} ${call.url.replace("http://127.0.0.1:8787/api/app-server", "")}`);
    expect(all).toEqual([
      "POST /initialize",
      "POST /initialized",
      "POST /conversations/c1/messages",
    ]);
    // The path param is consumed by the path, not echoed into the body.
    expect(businessCalls(calls)[0].body).toEqual({ content: "hi", idempotency_key: "k1" });
    expect(businessCalls(calls)[0].headers["x-app-server-connection-id"]).toBe("conn-1");
  });

  it("maps GET methods onto their route with query params", async () => {
    const { instance, calls } = transport();
    await instance.request("run/events", { run_id: "r1", after_sequence: 7, limit: 50 });
    const call = businessCalls(calls)[0];
    expect(call.method).toBe("GET");
    expect(call.url).toBe("http://127.0.0.1:8787/api/app-server/run/r1/events?after_sequence=7&limit=50");
    expect(call.body).toBeUndefined();
  });

  it("carries only the declared body fields and drops the path param", async () => {
    const { instance, calls } = transport();
    await instance.request("run/steer", {
      run_id: "r2",
      text: "go left",
      expected_version: 3,
      command_id: "cmd",
      idempotency_key: "k2",
      unexpected_field: "ignored",
    });
    const call = businessCalls(calls)[0];
    expect(call.url.endsWith("/run/r2/steer")).toBe(true);
    expect(call.body).toEqual({
      text: "go left",
      expected_version: 3,
      command_id: "cmd",
      idempotency_key: "k2",
    });
  });

  it("rejects missing path params instead of sending a broken URL", async () => {
    const { instance, calls } = transport();
    await expect(instance.request("conversation/get", {})).rejects.toBeInstanceOf(TransportError);
    expect(businessCalls(calls)).toHaveLength(0);
  });

  it("refuses WebSocket-only methods with an explanatory error", async () => {
    const { instance, calls } = transport();
    await expect(instance.request("conversation/subscribe", { conversation_id: "c1" })).rejects.toThrow(
      /WebSocket-only/,
    );
    // No handshake is spent on a method that has no binding.
    expect(calls).toHaveLength(0);
  });

  it("refuses methods with no HTTP binding, including the deliberate workspace/create gap", async () => {
    const { instance } = transport();
    await expect(instance.request("conversation/update", {})).rejects.toThrow(/no HTTP binding/);
    await expect(instance.request("workspace/create", { path: "/tmp/x" })).rejects.toThrow(
      /no HTTP binding/,
    );
  });

  it("throws on notify and hands back a no-op notification unsubscribe", () => {
    const { instance } = transport();
    expect(() => instance.notify("run/subscribe", {})).toThrow(/WebSocket-only/);
    // Documented non-equivalence: subscribing over HTTP yields no events at all.
    expect(typeof instance.onNotification(() => undefined)).toBe("function");
    expect(instance.onNotification(() => undefined)()).toBeUndefined();
  });

  it("surfaces the server wire error as an AppServerError", async () => {
    const { instance } = transport([
      {
        status: 403,
        body: {
          code: "not_initialized",
          message: "connection is not initialized",
          retryable: false,
          details: {},
          request_id: null,
        },
      },
    ]);
    await instance.request("skill/list", {}).catch((error: unknown) => {
      expect(error).toBeInstanceOf(AppServerError);
      const appError = error as AppServerError;
      expect(appError.code).toBe("not_initialized");
      expect(appError.retryable).toBe(false);
    });
  });

  it("falls back to a TransportError when the failure has no wire error body", async () => {
    const { instance } = transport([{ status: 503, body: { unexpected: true } }]);
    await expect(instance.request("models/list", {})).rejects.toMatchObject({
      name: "TransportError",
      retryable: true,
    });
  });

  it("strips a websocket endpoint or trailing slash into one base URL", async () => {
    const fake = fakeFetch();
    const fromWs = new HttpTransport({
      baseUrl: "http://127.0.0.1:8787/api/app-server/ws",
      fetch: fake.impl,
    });
    await fromWs.request("store/list", {});
    expect(businessCalls(fake.calls)[0].url).toBe("http://127.0.0.1:8787/api/app-server/store");
  });

  it("carries the route table with a verification source for every entry", () => {
    const table = httpRouteTable();
    const names = Object.keys(table);
    // Spot-check the shapes that the R2 reconnaissance had to read code for.
    expect(table["market/remove"].path).toBe("/markets/:marketplace_id/remove");
    expect(table["connector/auth/start"].path).toBe("/connectors/:connector_id/auth-start");
    expect(table["store/install-entry"].path).toBe(
      "/store/:marketplace_id/entries/:entry_name/install",
    );
    // The HTTP-only `POST /workspaces` register route is deliberately not a method.
    expect(names).not.toContain("workspace/create");
    for (const [method, route] of Object.entries(table)) {
      expect(route.source.trim().length, `${method} must record where it was verified`).toBeGreaterThan(5);
      expect(route.path.startsWith("/"), `${method} path must be absolute`).toBe(true);
    }
  });
});

describe("HttpTransport · shared surface (doc 16 R2)", () => {
  it("openConnection() is public so host-only routes can borrow a ready connection id", async () => {
    const fake = fakeFetch();
    const instance = new HttpTransport({
      baseUrl: "http://127.0.0.1:8787/api/app-server",
      fetch: fake.impl,
      client: { name: "webui", version: "1" },
      capabilities: { approvals: false },
    });
    await expect(instance.openConnection()).resolves.toBe("conn-1");
    const initialize = fake.calls.find((call) => call.url.endsWith("/initialize"));
    expect(initialize?.body).toMatchObject({
      protocol_version: expect.any(String),
      client: { name: "webui", version: "1" },
      capabilities: { approvals: false },
    });
  });

  it("appServerErrorFromWire is the single wire-error mapping for transport and host helpers", () => {
    const asAppError = appServerErrorFromWire(
      { code: "workspace_denied", message: "no", retryable: false, details: { a: 1 } },
      403,
    );
    expect(asAppError).toBeInstanceOf(AppServerError);
    expect((asAppError as AppServerError).code).toBe("workspace_denied");

    const asTransportError = appServerErrorFromWire(null, 502, "browse");
    expect(asTransportError).toMatchObject({ name: "TransportError", retryable: true });
    expect(String((asTransportError as Error).message)).toContain("browse");
    expect(appServerErrorFromWire(null, 400)).toMatchObject({ retryable: false });
  });
});

describe("HttpTransport · doc drift guard (site docs §7.3)", () => {
  /**
   * The developer guide states "covers 45 / 64 methods" and lists the 19
   * without an HTTP binding. Both numbers live in prose and in this table, so
   * mapping a new method (or dropping one) must fail this test and force the
   * guide to be updated in the same change.
   */
  const DOCUMENTED_UNMAPPED = [
    "initialize",
    "initialized",
    "workspace/create",
    "conversation/model-options",
    "conversation/update",
    "conversation/subscribe",
    "conversation/unsubscribe",
    "run/subscribe",
    "run/unsubscribe",
    "agent/list",
    "agent/get",
    "team/list",
    "team/get",
    // Host settings file (doc 16 R16): WebSocket-only host management surface,
    // like the catalogs above — and deliberately absent from this package's
    // client (the Web UI calls it through its own transport helpers).
    "config/get",
    "config/set",
    // Skill write face (doc 16 R17 / W12): the host's own CRUD over user
    // skills. A third-party consumer must not be able to write files into this
    // host's skill tree, so it is WebSocket-only and absent from this package
    // as well.
    "skill/create",
    "skill/update",
    "skill/delete",
    // Deriving a new user skill from any origin (2026-09-11): the same host-only
    // face, WebSocket-only for the same reason.
    "skill/copy",
  ];

  it("keeps the documented 45-mapped / 19-unmapped split", () => {
    const table = httpRouteTable();
    expect(Object.keys(table)).toHaveLength(45);
    expect(DOCUMENTED_UNMAPPED).toHaveLength(19);
    for (const method of DOCUMENTED_UNMAPPED) {
      expect(Object.keys(table), `${method} must stay unmapped`).not.toContain(method);
    }
  });

  it("binds run/plan to the shared owner-scoped executor", async () => {
    const { instance, calls } = transport();
    await instance.request("run/plan", { run_id: "r1" });
    const call = businessCalls(calls)[0];
    expect(call.method).toBe("GET");
    expect(call.url.endsWith("/run/r1/plan")).toBe(true);
    // The binding must document that it shares the WS arm's executor.
    expect(httpRouteTable()["run/plan"].source).toContain("get_run_plan_for_user");
  });

  it("binds run/answer-decision to the shared HTTP executor with no approve-all flag", async () => {
    const { instance, calls } = transport();
    await instance.request("run/answer-decision", {
      run_id: "r3",
      step_id: "0190f5fe-7c00-7a00-8000-000000000011",
      attempt_id: "0190f5fe-7c00-7a00-8000-000000000012",
      answer: "approved",
      expected_execution_version: 4,
      expected_step_version: 5,
      expected_attempt_version: 6,
      unexpected_field: "ignored",
    });
    const call = businessCalls(calls)[0];
    expect(call.method).toBe("POST");
    expect(call.url.endsWith("/run/r3/answer-decision")).toBe(true);
    // Exactly the protocol body: the path param is consumed by the path, the
    // three CAS tokens survive verbatim, and nothing else is invented.
    expect(call.body).toEqual({
      step_id: "0190f5fe-7c00-7a00-8000-000000000011",
      attempt_id: "0190f5fe-7c00-7a00-8000-000000000012",
      answer: "approved",
      expected_execution_version: 4,
      expected_step_version: 5,
      expected_attempt_version: 6,
    });
    // The binding must document that it shares the WS arm's executor.
    expect(httpRouteTable()["run/answer-decision"].source).toContain("execute_answer_decision");
  });

  it("documents the WebSocket-only set as exactly the subscription methods plus catalogs", () => {
    // Guarding the split above is what the guide quotes; this asserts the shape
    // of the two groups so a rename cannot silently move a method between them.
    expect(DOCUMENTED_UNMAPPED.filter((m) => m.includes("subscribe"))).toEqual([
      "conversation/subscribe",
      "conversation/unsubscribe",
      "run/subscribe",
      "run/unsubscribe",
    ]);
    expect(httpRouteTable()["conversation/create"]).toBeDefined();
  });
});
