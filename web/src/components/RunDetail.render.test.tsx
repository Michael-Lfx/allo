import { describe, expect, it, vi } from "vitest";

import type { RunEvent, RunPlan } from "../lib/protocol";

/**
 * Render smoke test for the W6 Run surface (R11).
 *
 * The projection is covered by `lib/run-tree.test.ts`; this file pins that the
 * *component* actually renders that projection (labels, statuses, retries,
 * approval question, collapsible raw pane) instead of crashing or going blank —
 * the failure mode a type check cannot see.
 */
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });

const [{ renderToStaticMarkup }, { createElement }, { RunSurface }] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./RunDetail"),
]);

function event(sequence: number, eventType: string, extra: Partial<RunEvent> = {}): RunEvent {
  return { run_id: "run-7", sequence, event_type: eventType, payload: {}, ...extra };
}

const EVENTS: RunEvent[] = [
  event(1, "run.started", { payload: { status: "planning" } }),
  event(2, "run.plan_changed", { payload: { status: "running", change: "initial_plan" } }),
  event(3, "run.status_changed", { payload: { status: "running", reason: "plan_approved" } }),
  event(4, "task.updated", { step_id: "step-alpha", payload: { change: "retry_requested" } }),
  event(5, "attempt.updated", {
    step_id: "step-alpha",
    attempt_id: "attempt-1",
    payload: { attempt_status: "failed", step_status: "running" },
  }),
  event(6, "attempt.updated", {
    step_id: "step-alpha",
    attempt_id: "attempt-2",
    payload: { attempt_status: "running", step_status: "running", control: "callback" },
  }),
  event(7, "approval.requested", {
    step_id: "step-alpha",
    attempt_id: "attempt-2",
    payload: { question: "May I rewrite the config?" },
    expected_execution_version: 3,
    expected_step_version: 2,
    expected_attempt_version: 1,
  }),
  event(8, "task.updated", { step_id: "step-beta", payload: { change: "conversation_effect_delivered", effect: "steer" } }),
  event(9, "run.status_changed", { payload: { status: "completed_with_failures" } }),
];

function render(activeRunId: string | null, plan: RunPlan | null = null): string {
  return renderToStaticMarkup(
    createElement(RunSurface, { runId: activeRunId, events: EVENTS, plan, onCancel: () => {} }),
  );
}

/** W4（R10）：`run/plan` 快照的渲染夹具——标题 / 成员 / 失败原因 / 耗时只有快照有。 */
const PLAN: RunPlan = {
  run_id: "run-7",
  status: "completed_with_failures",
  version: 3,
  dependencies: [{ blocker_step_id: "step-alpha", blocked_step_id: "step-beta" }],
  steps: [
    {
      step_id: "step-alpha",
      title: "重写审计配置",
      kind: "agent",
      status: "completed",
      role: "builder",
      model: "gpt-5",
      introduced_in_revision: 1,
      superseded_in_revision: null,
      created_at: 1,
      updated_at: 2,
      attempts: [{
        attempt_id: "attempt-1",
        attempt_no: 0,
        status: "failed",
        trigger_reason: "initial",
        role: "builder",
        model: "gpt-5",
        question: null,
        error: "provider 429",
        output_summary: null,
        output_files: ["out/report.md"],
        tokens: 512,
        started_at: 1000,
        finished_at: 1200,
      }],
    },
    {
      step_id: "step-beta",
      title: "写验收报告",
      kind: "verify",
      status: "running",
      role: null,
      model: null,
      introduced_in_revision: 2,
      superseded_in_revision: null,
      created_at: 3,
      updated_at: 4,
      attempts: [],
    },
  ],
};

describe("RunDetail (W6 run surface)", () => {
  it("renders nothing until a run is being followed", () => {
    expect(render(null)).toBe("");
  });

  it("renders the plan revision, the step tree, retries, the approval and the raw pane", () => {
    const html = render("run-7");

    expect(html).toContain("计划修订（1）");
    expect(html).toContain("初始计划");
    expect(html).toContain("原因：计划已批准");
    // Two steps, each rendered with its own status badge.
    expect(html).toContain("步骤（2）");
    expect(html).toContain("重试 1");
    expect(html).toContain("尝试 2");
    // step-alpha carries the nested attempts, the approval question and its state.
    expect(html).toContain("May I rewrite the config?");
    expect(html).toContain("待答复");
    // step-beta stays independent from step-alpha's attempts.
    expect(html).toContain("引导（steer）");
    // Terminal status drives the header badge and disables cancellation.
    expect(html).toContain("完成（含失败步骤）");
    expect(html).toContain("disabled");
    // The raw event log is demoted to a collapsed debug pane.
    expect(html).toContain("<details");
    expect(html).toContain("原始事件（调试，9）");
    expect(html).toContain("run.status_changed");
  });

  it("renders the plan/to-do block from the snapshot, with jump anchors on both sides", () => {
    const html = render("run-7", PLAN);

    expect(html).toContain("计划与待办（1/2 完成）");
    // 标题来自快照（事件里没有）。
    expect(html).toContain("重写审计配置");
    expect(html).toContain("写验收报告");
    // 成员归属 = role + model，不是内部 id。
    expect(html).toContain("builder · gpt-5");
    // 尝试行：序号按展示序（引擎 attempt_no 从 0 起）、原因、失败文本、耗时、token。
    expect(html).toContain("#1");
    expect(html).toContain("provider 429");
    expect(html).toContain("200ms");
    expect(html).toContain("512 tokens");
    // 修订号只在 >1 时出现，`superseded` 单独标注。
    expect(html).toContain("第 2 版引入");
    // 双向锚点：待办行与事件树步骤行各自带 id，互跳按钮指向对侧。
    expect(html).toContain('id="plan-step-step-alpha"');
    expect(html).toContain('id="run-step-step-alpha"');
    expect(html).toContain("定位到事件树");
    // 没有快照时不编内容，只说明快照不可用（事件树仍然在）。
    const withoutPlan = render("run-7");
    expect(withoutPlan).toContain("计划快照暂不可用");
    expect(withoutPlan).toContain("步骤（2）");
  });
});
