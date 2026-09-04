import { describe, expect, test } from "bun:test";

import {
  CANVAS_AGENT_ADVERTISED_TOOLS,
  CANVAS_AGENT_CONSTITUTION,
  CANVAS_AGENT_INCOMPLETE_NUDGE,
  CanvasHarness,
} from "./canvas-agent-harness";

describe("CanvasHarness", () => {
  const harness = new CanvasHarness({ maxSteps: 4 });

  test("owns constitution and advertised intent tools", () => {
    expect(harness.constitution()).toBe(CANVAS_AGENT_CONSTITUTION);
    expect(harness.constitution()).toContain("感知—行动—观察");
    expect([...CANVAS_AGENT_ADVERTISED_TOOLS]).toEqual([
      "canvas_list_skills",
      "canvas_get_skill",
      "canvas_inspect",
      "canvas_propose",
      "canvas_apply",
      "canvas_run",
      "canvas_critique",
      "canvas_repair",
    ]);
    expect(harness.advertiseTool("canvas_apply")).toBe(true);
    expect(harness.advertiseTool("canvas_apply_ops")).toBe(false);
    expect(harness.advertiseTool("Read")).toBe(false);
    expect(harness.isReadTool("canvas_inspect")).toBe(true);
    expect(harness.isReadTool("canvas_apply")).toBe(false);
  });

  test("first turn requires a tool call; later turns are auto", () => {
    expect(harness.firstTurnToolChoice()).toBe("required");
    expect(harness.decideAfterTools(1)).toEqual({ action: "call_model", toolChoice: "auto" });
  });

  test("hard-stops at max steps after tools", () => {
    expect(harness.decideAfterTools(4)).toEqual({ action: "hard_stop", reason: "max_steps" });
  });

  test("forces another tool when the generation queue is still incomplete", () => {
    expect(harness.decideAfterModel({
      step: 2,
      toolCallCount: 0,
      writableCallCount: 0,
      confirmTools: false,
      incomplete: true,
    })).toEqual({ action: "force_tool", nudge: CANVAS_AGENT_INCOMPLETE_NUDGE });
  });

  test("ends when the model replies without tools and the queue is idle", () => {
    expect(harness.decideAfterModel({
      step: 2,
      toolCallCount: 0,
      writableCallCount: 0,
      confirmTools: false,
      incomplete: false,
    })).toEqual({ action: "end" });
  });

  test("pauses writable tools for host confirmation", () => {
    expect(harness.decideAfterModel({
      step: 1,
      toolCallCount: 2,
      writableCallCount: 1,
      confirmTools: true,
      skipConfirm: false,
      incomplete: false,
    })).toEqual({ action: "await_confirm" });
    expect(harness.decideAfterModel({
      step: 1,
      toolCallCount: 2,
      writableCallCount: 1,
      confirmTools: true,
      skipConfirm: true,
      incomplete: false,
    })).toEqual({ action: "run_tools" });
  });
});
