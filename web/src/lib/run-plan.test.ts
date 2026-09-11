import { describe, expect, it } from "vitest";

import type { RunPlan } from "./protocol";
import {
  attemptDurationMs,
  formatDuration,
  memberLabel,
  planProgress,
  planStepAnchor,
  planStepTone,
  planStepViews,
  runStepAnchor,
} from "./run-plan";

function attempt(overrides: Partial<RunPlan["steps"][number]["attempts"][number]> = {}) {
  return {
    attempt_id: "a1",
    attempt_no: 1,
    status: "completed",
    trigger_reason: "initial",
    output_files: [],
    ...overrides,
  } as RunPlan["steps"][number]["attempts"][number];
}

function plan(steps: RunPlan["steps"] = []): RunPlan {
  return { run_id: "r1", status: "running", version: 3, steps, dependencies: [] };
}

const STEP = {
  step_id: "s1",
  title: "抓取官网文档",
  kind: "agent",
  status: "completed",
  role: "builder",
  model: "gpt-5",
  introduced_in_revision: 1,
  created_at: 1,
  updated_at: 2,
  attempts: [],
};

describe("run-plan · 快照投影", () => {
  it("把快照折成待办行：序号 / 标题 / 色调 / 成员 / 修订号", () => {
    const views = planStepViews(plan([STEP, { ...STEP, step_id: "s2", title: "写报告", status: "running", model: null }]));
    expect(views.map((view) => view.index)).toEqual([1, 2]);
    expect(views[0]).toMatchObject({ title: "抓取官网文档", tone: "ok", member: "builder · gpt-5", introducedInRevision: 1 });
    expect(views[1]).toMatchObject({ tone: "active", member: "builder" });
  });

  it("空快照返回空数组（界面走空态，不造占位行）", () => {
    expect(planStepViews(null)).toEqual([]);
    expect(planStepViews(plan([]))).toEqual([]);
  });

  it("标题缺失时退回 step_id，不显示空白行", () => {
    const views = planStepViews(plan([{ ...STEP, title: "   " }]));
    expect(views[0].title).toBe("s1");
  });

  it("被修订取代的步骤仍可追溯，但不计入待办进度", () => {
    const views = planStepViews(plan([
      { ...STEP, step_id: "s1", superseded_in_revision: 2, status: "completed" },
      { ...STEP, step_id: "s2", status: "running" },
    ]));
    expect(views[0].superseded).toBe(true);
    expect(planProgress(views)).toEqual({ done: 0, total: 1 });
  });

  it("进度按未取代步骤里 done 的数量统计", () => {
    const views = planStepViews(plan([
      { ...STEP, step_id: "s1", status: "completed" },
      { ...STEP, step_id: "s2", status: "failed" },
      { ...STEP, step_id: "s3", status: "pending" },
    ]));
    expect(planProgress(views)).toEqual({ done: 1, total: 3 });
  });
});

describe("run-plan · 尝试细节", () => {
  it("耗时只在两端都有时间戳时给出", () => {
    expect(attemptDurationMs({ started_at: 1000, finished_at: 4200 })).toBe(3200);
    expect(attemptDurationMs({ started_at: 1000, finished_at: null })).toBeNull();
    expect(attemptDurationMs({ started_at: null, finished_at: 2000 })).toBeNull();
    // 时钟回拨：给出负数比给出 0 更坏，一律当作未知。
    expect(attemptDurationMs({ started_at: 5000, finished_at: 1000 })).toBeNull();
  });

  it("人话时长分档；未知返回 null", () => {
    expect(formatDuration(820)).toBe("820ms");
    expect(formatDuration(3400)).toBe("3.4s");
    expect(formatDuration(125_000)).toBe("2m 05s");
    expect(formatDuration(null)).toBeNull();
    expect(formatDuration(-1)).toBeNull();
  });

  it("成员归属只在有值时才拼，缺一个不补「未知」", () => {
    expect(memberLabel("builder", "gpt-5")).toBe("builder · gpt-5");
    expect(memberLabel("builder", null)).toBe("builder");
    expect(memberLabel(null, "  ")).toBeNull();
  });

  it("尝试行带上原因 / 错误 / 产物 / token，并保留空错误为 null", () => {
    const views = planStepViews(plan([{
      ...STEP,
      attempts: [
        attempt({ attempt_id: "a1", attempt_no: 0, status: "failed", trigger_reason: "initial", error: "provider 429", started_at: 1000, finished_at: 1200, tokens: 512 }),
        attempt({ attempt_id: "a2", attempt_no: 1, status: "running", trigger_reason: "retry_requested", error: "   ", output_files: ["out/report.md"] }),
      ],
    }]));
    expect(views[0].attempts.map((a) => a.attemptId)).toEqual(["a1", "a2"]);
    // 展示序号从 1 起，与引擎从 0 起的 attempt_no 解耦。
    expect(views[0].attempts.map((a) => a.ordinal)).toEqual([1, 2]);
    expect(views[0].attempts.map((a) => a.attemptNo)).toEqual([0, 1]);
    expect(views[0].attempts[0]).toMatchObject({ tone: "bad", error: "provider 429", durationMs: 200, tokens: 512 });
    // 空错误不当成错误文本，进行中的尝试没有时长。
    expect(views[0].attempts[1]).toMatchObject({ error: null, durationMs: null, outputFiles: ["out/report.md"] });
    expect(views[0].attempts[1].triggerReason).toBe("retry_requested");
  });

  it("superseded / skipped 用最弱的一档，不冒充成功或失败", () => {
    expect(planStepTone("superseded")).toBe("pending");
    expect(planStepTone("skipped")).toBe("pending");
    expect(planStepTone("waiting_input")).toBe("attention");
    expect(planStepTone("failed")).toBe("bad");
  });

  it("两侧锚点 id 不同，互跳不会自己指向自己", () => {
    expect(planStepAnchor("s1")).toBe("plan-step-s1");
    expect(runStepAnchor("s1")).toBe("run-step-s1");
  });
});
