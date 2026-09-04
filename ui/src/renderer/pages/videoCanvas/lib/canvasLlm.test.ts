import { describe, expect, test } from "bun:test";

import { isCanvasLlmTransportFailure, toCanvasLlmTransportError } from "./canvasLlm";

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
