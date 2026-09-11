import { afterEach, describe, expect, it, vi } from "vitest";

import { AppServerClient } from "./client";

/**
 * R15（W10）拖拽 / 粘贴的字节通道：`uploadFileToWorkspace`。
 *
 * 这里钉两件事：
 *  - 走的是宿主文件服务的根路径 `/api/fs/upload`（不是 App Server 的 `/api/app-server`）；
 *  - 请求体是 multipart，且带上 `workspace` 落点（服务端据此把字节写进会话工作区，
 *    而不是宿主 tmp——落 tmp 会得到发送路径必然拒绝的路径）。
 */
const CONNECTION_HEADER = "x-app-server-connection-id";

function jsonResponse(body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json", ...headers },
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("AppServerClient · uploadFileToWorkspace（R15）", () => {
  it("POST /api/fs/upload，multipart 带 workspace 与文件，返回落点路径", async () => {
    const calls: { url: string; body: unknown }[] = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      const url =
        typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
      calls.push({ url, body: init?.body });
      if (url.endsWith("/initialize")) {
        return jsonResponse({ data: {} }, { [CONNECTION_HEADER]: "conn-1" });
      }
      if (url.endsWith("/initialized")) return jsonResponse({ data: {} });
      if (url.endsWith("/api/fs/upload")) return jsonResponse({ success: true, data: "C:/ws/drop.png" });
      return new Response("not found", { status: 404 });
    });

    const client = new AppServerClient({
      httpBaseUrl: "http://127.0.0.1:8787/api/app-server",
      wsUrl: "ws://127.0.0.1:8787/api/app-server/ws",
    });
    const file = new File([new Uint8Array([1, 2, 3])], "drop.png", { type: "image/png" });

    await expect(client.uploadFileToWorkspace("C:/ws", file)).resolves.toBe("C:/ws/drop.png");

    const upload = calls.find((call) => call.url.endsWith("/api/fs/upload"));
    expect(upload, "upload must hit the host root path, not /api/app-server").toBeDefined();
    const form = upload!.body as FormData;
    expect(form.get("workspace")).toBe("C:/ws");
    expect(form.get("file_name")).toBe("drop.png");
    expect(form.get("file")).toBeInstanceOf(File);
  });
});
