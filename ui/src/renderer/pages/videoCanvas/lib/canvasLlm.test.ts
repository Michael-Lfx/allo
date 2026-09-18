import { describe, expect, test } from "bun:test";

import { isCanvasLlmTransportFailure, toCanvasLlmTransportError, finalizeCanvasChatToolCalls, streamCanvasChatCompletions, textFromChatContent } from "./canvasLlm";

describe("canvas LLM transport errors", () => {
  test("maps fetch/network failures to a planner-facing message", () => {
    expect(isCanvasLlmTransportFailure(new TypeError("Failed to fetch"))).toBe(true);
    expect(isCanvasLlmTransportFailure(new Error("Network Error"))).toBe(true);
    expect(toCanvasLlmTransportError(new TypeError("Failed to fetch")).message).toContain("规划模型请求失败");
  });

  test("leaves abort and ordinary errors alone", () => {
    expect(isCanvasLlmTransportFailure(new Error("鉴权失败"))).toBe(false);
    const abort = new DOMException("Aborted", "AbortError");
    expect(toCanvasLlmTransportError(abort).message).toBe("请求已取消");
  });
});

describe("canvas LLM tool calls", () => {
  test("keeps function calls even when the provider omits id", () => {
    const calls = finalizeCanvasChatToolCalls([
      { id: "", name: "review_art_composition", arguments: "{\"candidates\":[]}" },
      { name: "review_art_color", arguments: "{}" },
    ]);
    expect(calls).toHaveLength(2);
    expect(calls[0].function.name).toBe("review_art_composition");
    expect(calls[0].id).toBeTruthy();
    expect(calls[1].function.name).toBe("review_art_color");
  });

  test("reads array-shaped chat content", () => {
    expect(textFromChatContent([{ type: "text", text: "hello" }, { type: "text", text: " world" }])).toBe("hello world");
  });

  test("parses non-stream JSON tool calls without ids", async () => {
    const originalFetch = globalThis.fetch;
    globalThis.fetch = (async () =>
      new Response(
        JSON.stringify({
          choices: [
            {
              message: {
                content: "",
                tool_calls: [{ function: { name: "review_art_composition", arguments: "{\"candidates\":[]}" } }],
              },
            },
          ],
        }),
        { headers: { "content-type": "application/json" } },
      )) as typeof fetch;
    try {
      const result = await streamCanvasChatCompletions({ model: "test", messages: [] }, { stream: false });
      expect(result.toolCalls).toHaveLength(1);
      expect(result.toolCalls[0].function.name).toBe("review_art_composition");
      expect(result.toolCalls[0].id).toBeTruthy();
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  test("parses SSE snapshots that send message.tool_calls instead of delta", async () => {
    const originalFetch = globalThis.fetch;
    const payload = {
      choices: [
        {
          message: {
            content: "",
            tool_calls: [{ index: 0, function: { name: "analyze_art_scene", arguments: "{\"scene\":{}}" } }],
          },
        },
      ],
    };
    globalThis.fetch = (async () =>
      new Response(`data: ${JSON.stringify(payload)}\n\ndata: [DONE]\n\n`, { headers: { "content-type": "text/event-stream" } })) as typeof fetch;
    try {
      const result = await streamCanvasChatCompletions({ model: "test", messages: [] });
      expect(result.toolCalls.map((call) => call.function.name)).toEqual(["analyze_art_scene"]);
      expect(result.toolCalls[0].function.arguments).toContain("scene");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
