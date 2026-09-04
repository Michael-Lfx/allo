import { describe, expect, test } from "bun:test";

import { toCanvasChatMessages } from "./canvas-agent-llm";

describe("canvas agent LLM runtime", () => {
  test("packs function_call batches into one assistant tool_calls message", () => {
    const messages = toCanvasChatMessages([
      { role: "system", content: "constitution" },
      { role: "user", content: "小猫的一天" },
      { type: "function_call", call_id: "1", name: "canvas_inspect", arguments: "{}" },
      { type: "function_call", call_id: "2", name: "canvas_apply", arguments: "{}" },
      { role: "tool", tool_call_id: "1", content: "{\"ok\":true}" },
    ]);
    expect(messages[0]).toEqual({ role: "system", content: "constitution" });
    expect(messages[2]).toEqual({
      role: "assistant",
      content: null,
      tool_calls: [
        { id: "1", type: "function", function: { name: "canvas_inspect", arguments: "{}" } },
        { id: "2", type: "function", function: { name: "canvas_apply", arguments: "{}" } },
      ],
    });
    expect(messages[3]).toEqual({ role: "tool", tool_call_id: "1", content: "{\"ok\":true}" });
  });
});
