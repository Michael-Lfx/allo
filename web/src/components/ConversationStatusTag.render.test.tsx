import { describe, expect, it, vi } from "vitest";

import type { RunEvent } from "../lib/protocol";

/**
 * Render smoke test for the sidebar conversation status tag.
 *
 * `lib/conversation-status.test.ts` pins the derivation rules; this file pins
 * that the *component* renders them into the row (a label, not another wordless
 * dot) — the failure mode a type check cannot see.
 */
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("navigator", { language: "zh-CN" });

// 与 RunDetail.render.test 同款：i18n 必须初始化，否则 `t()` 原样返回 key。
await import("../i18n");

const [{ renderToStaticMarkup }, { createElement }, { ConversationStatusTag }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./ConversationStatusTag"),
]);

function render(input: {
  isProcessing: boolean;
  runConversationId?: string | null;
  activeRunId?: string | null;
  runEvents?: RunEvent[];
}): string {
  return renderToStaticMarkup(
    createElement(ConversationStatusTag, {
      conversationId: "conv-1",
      isProcessing: input.isProcessing,
      runConversationId: input.runConversationId ?? null,
      activeRunId: input.activeRunId ?? null,
      runEvents: input.runEvents ?? [],
    }),
  );
}

const started: RunEvent = {
  run_id: "run-1",
  sequence: 1,
  event_type: "run.started",
  payload: { status: "planning" },
};

describe("ConversationStatusTag", () => {
  it("renders the followed Run's status label instead of a bare dot", () => {
    const html = render({
      isProcessing: true,
      runConversationId: "conv-1",
      activeRunId: "run-1",
      runEvents: [started],
    });
    expect(html).toContain("规划中");
    expect(html).toContain("conversation-status-tag is-active");
  });

  it("falls back to the processing label while a turn runs without a Run status", () => {
    const html = render({ isProcessing: true });
    expect(html).toContain("正在处理");
    expect(html).toContain("conversation-status-tag is-active");
  });

  it("renders nothing for a quiet row", () => {
    expect(render({ isProcessing: false })).toBe("");
  });

  it("does not tag a row with someone else's followed Run", () => {
    const html = render({
      isProcessing: false,
      runConversationId: "conv-2",
      activeRunId: "run-1",
      runEvents: [started],
    });
    expect(html).toBe("");
  });
});
