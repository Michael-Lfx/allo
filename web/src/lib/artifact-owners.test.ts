import { describe, expect, it } from "vitest";

import type { RunPlan } from "./protocol";
import { collectArtifactOwners, normalizeArtifactKey } from "./artifact-owners";

/**
 * R20a：产物归属投影的口径。
 *
 * Behaviour pinned here:
 *  - 路径归一化只做无歧义的部分（分隔符 / `./` / 首尾空白），**不折叠大小写**；
 *  - 同一条路径只认**最先出现**的那次尝试（产物是它先产出的）；
 *  - 没有快照 / 没有 run id 时返回空表，不造占位归属。
 */

function attempt(overrides: Partial<RunPlan["steps"][number]["attempts"][number]> = {}) {
  return {
    attempt_id: "a1",
    attempt_no: 0,
    status: "completed",
    trigger_reason: "initial",
    output_files: [],
    ...overrides,
  } as RunPlan["steps"][number]["attempts"][number];
}

function step(overrides: Partial<RunPlan["steps"][number]> = {}): RunPlan["steps"][number] {
  return {
    step_id: "s1",
    title: "抓取官网文档",
    kind: "agent",
    status: "completed",
    introduced_in_revision: 1,
    created_at: 1,
    updated_at: 2,
    attempts: [],
    ...overrides,
  } as RunPlan["steps"][number];
}

function plan(steps: RunPlan["steps"]): RunPlan {
  return { run_id: "run-1", status: "completed", version: 3, steps, dependencies: [] };
}

describe("normalizeArtifactKey", () => {
  it("归一分隔符与 ./ 前缀，让快照与列表能对上", () => {
    expect(normalizeArtifactKey("./out/report.md")).toBe("out/report.md");
    expect(normalizeArtifactKey("out\\report.md")).toBe("out/report.md");
    expect(normalizeArtifactKey("/out/report.md")).toBe("out/report.md");
    expect(normalizeArtifactKey("  out/report.md  ")).toBe("out/report.md");
  });

  it("不折叠大小写（宁可漏认也不误认）", () => {
    expect(normalizeArtifactKey("Out/Report.md")).toBe("Out/Report.md");
    expect(normalizeArtifactKey("Out/Report.md")).not.toBe(normalizeArtifactKey("out/report.md"));
  });

  it("空路径返回 null，不参与匹配", () => {
    expect(normalizeArtifactKey("")).toBeNull();
    expect(normalizeArtifactKey("   ")).toBeNull();
    expect(normalizeArtifactKey("./")).toBeNull();
    expect(normalizeArtifactKey(null)).toBeNull();
    expect(normalizeArtifactKey(undefined)).toBeNull();
  });
});

describe("collectArtifactOwners", () => {
  it("没有快照 / 没有 run id 时是空表", () => {
    expect(collectArtifactOwners(null, "run-1")).toEqual({});
    expect(collectArtifactOwners(undefined, "run-1")).toEqual({});
    expect(collectArtifactOwners(plan([]), "run-1")).toEqual({});
    expect(collectArtifactOwners(plan([step()]), "")).toEqual({});
  });

  it("按 step / attempt 展开产物，标题取快照标题", () => {
    const owners = collectArtifactOwners(
      plan([
        step({
          step_id: "step-a",
          title: "写报告",
          attempts: [attempt({ output_files: ["out/report.md", "./out/notes.md"] })],
        }),
        step({
          step_id: "step-b",
          title: "   ",
          attempts: [attempt({ attempt_id: "a2", attempt_no: 1, output_files: ["out/other.md"] })],
        }),
      ]),
      "run-7",
    );

    expect(owners["out/report.md"]).toEqual({
      runId: "run-7",
      stepId: "step-a",
      stepTitle: "写报告",
      attemptNo: 0,
    });
    expect(owners["out/notes.md"]?.stepId).toBe("step-a");
    // 快照标题为空时回落 step id（与 planStepViews 同口径），不编标题。
    expect(owners["out/other.md"]).toMatchObject({ stepId: "step-b", stepTitle: "step-b", attemptNo: 1 });
  });

  it("同一条路径保留最先出现的那次尝试，不被后续覆盖", () => {
    const owners = collectArtifactOwners(
      plan([
        step({
          step_id: "step-first",
          title: "第一次",
          attempts: [attempt({ attempt_id: "a1", attempt_no: 0, output_files: ["out/same.md"] })],
        }),
        step({
          step_id: "step-later",
          title: "重试后",
          attempts: [attempt({ attempt_id: "a2", attempt_no: 1, output_files: ["out/same.md"] })],
        }),
      ]),
      "run-7",
    );

    expect(owners["out/same.md"]).toMatchObject({ stepId: "step-first", attemptNo: 0 });
  });
});
