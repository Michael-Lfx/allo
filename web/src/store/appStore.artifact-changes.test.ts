import { beforeEach, describe, expect, it, vi } from "vitest";

import type { FileChangeInfo, SnapshotCompare } from "../lib/artifact-changes";

/**
 * R20b（W5 余项）的 store 侧：工作区变更的「接受 / 回退」。
 *
 * Behaviour pinned here:
 *  - `compare` 成功即用（排序后）写入，**不**多调一次 `init` 推高引用计数；
 *  - `compare` 抛「未初始化」才 `init` 一次建立基线，再重试 `compare`；
 *  - `init` 返回 `disabled` → 保留 reason、空态、不报错；
 *  - 接受 / 回退 / 全部接受 / 撤销接受都走相对路径，成功后重读；
 *  - 失败写进 `artifactChangesError` 并复位 busy；busy 期间重入被忽略。
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

const ROOT = "C:/ws/proj";

function change(relative: string, operation: FileChangeInfo["operation"] = "modify"): FileChangeInfo {
  return { file_path: `${ROOT}/${relative}`, relative_path: relative, operation };
}

const EMPTY: SnapshotCompare = { staged: [], unstaged: [] };

function fakeClient(overrides: Record<string, unknown> = {}) {
  const calls: string[] = [];
  const client = {
    snapshotCompare: async (workspace: string): Promise<SnapshotCompare> => {
      calls.push(`compare:${workspace}`);
      return { staged: [], unstaged: [change("z.txt"), change("a.txt", "create")] };
    },
    snapshotInit: async (workspace: string) => {
      calls.push(`init:${workspace}`);
      return { mode: "snapshot", branch: null, reason: null };
    },
    snapshotStageFile: async (_workspace: string, path: string) => {
      calls.push(`stage:${path}`);
    },
    snapshotStageAll: async (workspace: string) => {
      calls.push(`stage-all:${workspace}`);
    },
    snapshotUnstageFile: async (_workspace: string, path: string) => {
      calls.push(`unstage:${path}`);
    },
    snapshotDiscardFile: async (_workspace: string, path: string, operation: string) => {
      calls.push(`discard:${path}:${operation}`);
    },
    ...overrides,
  };
  return { calls, client };
}

function reset() {
  useAppStore.setState({
    client: null,
    artifactsRoot: null,
    artifactChanges: EMPTY,
    artifactChangesLoading: false,
    artifactChangesError: null,
    artifactSnapshot: null,
    artifactChangeBusy: null,
  });
}

describe("appStore · refreshArtifactChanges（R20b）", () => {
  beforeEach(reset);

  it("compare 成功：排序后写入两组变更，且不调 init", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().refreshArtifactChanges();
    const state = useAppStore.getState();
    expect(state.artifactChanges.unstaged.map((entry) => entry.relative_path)).toEqual(["a.txt", "z.txt"]);
    expect(fake.calls.filter((call) => call.startsWith("init:"))).toEqual([]);
    expect(state.artifactChangesError).toBeNull();
  });

  it("compare 抛「未初始化」→ init 一次建立基线后重试", async () => {
    let attempts = 0;
    const fake = fakeClient({
      snapshotCompare: async (workspace: string) => {
        attempts += 1;
        if (attempts === 1) throw new Error("Workspace not initialized");
        return { staged: [], unstaged: [change("a.txt")] };
      },
    });
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().refreshArtifactChanges();
    expect(attempts).toBe(2);
    expect(fake.calls).toContain(`init:${ROOT}`);
    expect(useAppStore.getState().artifactChanges.unstaged).toHaveLength(1);
  });

  it("init 返回 disabled：保留 reason、空态、不报错、不再 compare", async () => {
    let compares = 0;
    const fake = fakeClient({
      snapshotCompare: async () => {
        compares += 1;
        throw new Error("Workspace not initialized");
      },
      snapshotInit: async () => ({ mode: "disabled", branch: null, reason: "drive root" }),
    });
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().refreshArtifactChanges();
    expect(compares).toBe(1);
    const state = useAppStore.getState();
    expect(state.artifactSnapshot).toMatchObject({ mode: "disabled", reason: "drive root" });
    expect(state.artifactChanges).toEqual(EMPTY);
    expect(state.artifactChangesError).toBeNull();
  });

  it("没有 client / 没有工作区时清空并 no-op", async () => {
    await useAppStore.getState().refreshArtifactChanges();
    expect(useAppStore.getState().artifactChanges).toEqual(EMPTY);
    expect(useAppStore.getState().artifactSnapshot).toBeNull();
  });
});

describe("appStore · 变更动作（R20b）", () => {
  beforeEach(reset);

  it("接受一条 → stage 相对路径后重读 compare", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().acceptArtifactChange(change("a.txt", "create"));
    expect(fake.calls).toContain("stage:a.txt");
    expect(fake.calls.some((call) => call.startsWith("compare:"))).toBe(true);
    expect(useAppStore.getState().artifactChangeBusy).toBeNull();
  });

  it("回退一条 → discard 原样带上 operation", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().revertArtifactChange(change("b.txt", "create"));
    expect(fake.calls).toContain("discard:b.txt:create");
  });

  it("全部接受 → stage-all 一次请求", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().acceptAllArtifactChanges();
    expect(fake.calls).toContain(`stage-all:${ROOT}`);
  });

  it("撤销接受 → unstage 相对路径", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().unstageArtifactChange(change("c.txt"));
    expect(fake.calls).toContain("unstage:c.txt");
  });

  it("动作失败：错误写进 artifactChangesError，busy 复位", async () => {
    const fake = fakeClient({
      snapshotStageFile: async () => {
        throw new Error("403");
      },
    });
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT });
    await useAppStore.getState().acceptArtifactChange(change("a.txt"));
    const state = useAppStore.getState();
    expect(state.artifactChangesError).toContain("403");
    expect(state.artifactChangeBusy).toBeNull();
  });

  it("busy 期间忽略重入", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT, artifactChangeBusy: "a.txt" });
    await useAppStore.getState().acceptArtifactChange(change("a.txt"));
    expect(fake.calls).toEqual([]);
  });
});
