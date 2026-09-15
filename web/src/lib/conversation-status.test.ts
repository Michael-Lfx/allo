import { describe, expect, it } from "vitest";

import { conversationStatusTag, followedRunStatus } from "./conversation-status";
import type { RunEvent } from "./protocol";

const runEvent = (sequence: number, eventType: string, status: string): RunEvent => ({
  run_id: "run-1",
  sequence,
  event_type: eventType,
  payload: { status },
});

describe("conversationStatusTag", () => {
  it("prefers the followed Run's own status over the generic processing label", () => {
    expect(conversationStatusTag({ isProcessing: true, runStatus: "awaiting_approval" })).toEqual({
      labelKey: "run.statusValue.awaiting_approval",
      tone: "attention",
    });
  });

  it("falls back to the generic label when there is no Run status", () => {
    expect(conversationStatusTag({ isProcessing: true, runStatus: null })).toEqual({
      labelKey: "common.processing",
      tone: "active",
    });
  });

  it("shows no tag for a quiet row", () => {
    // 「仅活跃会话显示 tag」：历史行不编造状态（ConversationStatus 只有
    // pending/running/finished，finished 无法区分成功与失败）。
    expect(conversationStatusTag({ isProcessing: false, runStatus: null })).toBeNull();
  });

  it("drops the tag once the Run reaches a terminal status", () => {
    for (const status of ["completed", "completed_with_failures", "failed", "cancelled"]) {
      expect(conversationStatusTag({ isProcessing: true, runStatus: status })).toEqual({
        labelKey: "common.processing",
        tone: "active",
      });
    }
    expect(conversationStatusTag({ isProcessing: false, runStatus: "failed" })).toBeNull();
  });
});

describe("followedRunStatus", () => {
  it("returns the live status only for the conversation that owns the Run", () => {
    const events = [runEvent(1, "run.started", "planning")];
    expect(followedRunStatus("conv-1", "conv-1", "run-1", events)).toBe("planning");
    expect(followedRunStatus("conv-2", "conv-1", "run-1", events)).toBeNull();
  });

  it("returns null when nothing is followed or the Run is terminal", () => {
    const events = [runEvent(1, "run.started", "planning")];
    expect(followedRunStatus("conv-1", null, null, events)).toBeNull();
    expect(followedRunStatus("conv-1", "conv-1", "run-1", [])).toBeNull();
    const terminal = [...events, runEvent(2, "run.status_changed", "completed")];
    expect(followedRunStatus("conv-1", "conv-1", "run-1", terminal)).toBeNull();
  });
});
