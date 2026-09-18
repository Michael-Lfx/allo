import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * R20a（W5 余项 · 按 Run 归属）的 store 侧。
 *
 * Behaviour pinned here:
 *  - 归属只来自 `run/plan` 快照，并且只记进**发起该 Run 的会话**；
 *  - Run 不属于当前选中会话时一条都不记（不把别处的归属贴过来）；
 *  - 同会话的多次快照按路径累积，同路径由**更新的快照**覆盖；
 *  - 点归属标签：关抽屉（含预览），并在跟随的 Run 不同时重新 `followRun`。
 */
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true });
vi.stubGlobal("requestAnimationFrame", (run: () => void) => {
  run();
  return 0;
});

const { useAppStore } = await import("./appStore");

/** 面板里已打开的一个预览（用来验证跳转时预览也被清掉）。 */
const OPEN_PREVIEW = { path: "out/x.md", name: "x.md", content: null, loading: false, error: null };

function planWith(runId: string, stepId: string, files: string[]) {
  return {
    run_id: runId,
    status: "completed",
    version: 2,
    steps: [
      {
        step_id: stepId,
        title: "写报告",
        kind: "agent",
        status: "completed",
        introduced_in_revision: 1,
        created_at: 1,
        updated_at: 2,
        attempts: [
          { attempt_id: "at-1", attempt_no: 0, status: "completed", trigger_reason: "initial", output_files: files },
        ],
      },
    ],
    dependencies: [],
  } as never;
}

function fakeClient(plan: unknown) {
  return { runs: { plan: async () => plan } } as never;
}

beforeEach(() => {
  useAppStore.setState({
    client: null,
    activeRunId: null,
    runConversationId: null,
    selectedConversationId: null,
    runPlan: null,
    runPlanError: null,
    artifactPanelOpen: false,
    artifactPreview: null,
    artifactOwners: { conversationId: null, byPath: {} },
  });
});

describe("appStore · 产物归属（R20a）", () => {
  it("计划快照落库时把归属记进发起该 Run 的会话", async () => {
    useAppStore.setState({
      client: fakeClient(planWith("run-9", "step-a", ["out/report.md"])),
      activeRunId: "run-9",
      runConversationId: "conv-1",
      selectedConversationId: "conv-1",
    });

    await useAppStore.getState().loadRunPlan("run-9");

    const { conversationId, byPath } = useAppStore.getState().artifactOwners;
    expect(conversationId).toBe("conv-1");
    expect(byPath["out/report.md"]).toMatchObject({ runId: "run-9", stepId: "step-a", stepTitle: "写报告" });
  });

  it("Run 不属于当前选中会话时一条都不记", async () => {
    useAppStore.setState({
      client: fakeClient(planWith("run-9", "step-a", ["out/report.md"])),
      activeRunId: "run-9",
      runConversationId: "conv-other",
      selectedConversationId: "conv-1",
    });

    await useAppStore.getState().loadRunPlan("run-9");

    expect(useAppStore.getState().artifactOwners).toEqual({ conversationId: null, byPath: {} });
  });

  it("同会话多次快照按路径累积，同路径由更新的快照覆盖", async () => {
    useAppStore.setState({
      client: fakeClient(planWith("run-9", "step-a", ["out/a.md"])),
      activeRunId: "run-9",
      runConversationId: "conv-1",
      selectedConversationId: "conv-1",
    });
    await useAppStore.getState().loadRunPlan("run-9");

    // 第二次跟随后端上的另一个 Run：新路径并集进来，同路径换成新归属。
    useAppStore.setState({
      client: fakeClient(planWith("run-10", "step-b", ["out/a.md", "out/b.md"])),
      activeRunId: "run-10",
    });
    await useAppStore.getState().loadRunPlan("run-10");

    const { byPath } = useAppStore.getState().artifactOwners;
    expect(Object.keys(byPath).sort()).toEqual(["out/a.md", "out/b.md"]);
    expect(byPath["out/a.md"]).toMatchObject({ runId: "run-10", stepId: "step-b" });
  });

  it("点归属标签：关抽屉 + 重新跟随那个 Run", async () => {
    const followed: string[] = [];
    useAppStore.setState({
      activeRunId: "run-other",
      artifactPanelOpen: true,
      artifactPreview: OPEN_PREVIEW as never,
      artifactOwners: { conversationId: "conv-1", byPath: {} },
      followRun: (async (runId: string, conversationId?: string | null) => {
        followed.push(`${runId}:${conversationId}`);
      }) as never,
    });

    await useAppStore
      .getState()
      .focusArtifactOwner({ runId: "run-9", stepId: "step-a", stepTitle: "写报告", attemptNo: 0 });

    expect(followed).toEqual(["run-9:conv-1"]);
    expect(useAppStore.getState().artifactPanelOpen).toBe(false);
    expect(useAppStore.getState().artifactPreview).toBeNull();
  });

  it("已经在跟随那个 Run 时不再重复 follow", async () => {
    const followed: string[] = [];
    useAppStore.setState({
      activeRunId: "run-9",
      artifactOwners: { conversationId: "conv-1", byPath: {} },
      followRun: (async (runId: string) => {
        followed.push(runId);
      }) as never,
    });

    await useAppStore
      .getState()
      .focusArtifactOwner({ runId: "run-9", stepId: "step-a", stepTitle: "写报告", attemptNo: 0 });

    expect(followed).toEqual([]);
  });
});
