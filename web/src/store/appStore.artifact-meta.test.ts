import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * R20（W5 余项）的 store 侧：产物列表的 size / MIME / mtime 补齐。
 *
 * Behaviour pinned here:
 *  - 每个路径只查一次（成功与失败都记账），重复调用不再发请求；
 *  - 单条失败降级成 `null`，**不影响列表、不弹错、不重试**；
 *  - 未提供 client / 没有路径时是 no-op。
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

function file(name: string) {
  return { name, full_path: `${ROOT}/${name}`, relative_path: name };
}

function fakeClient() {
  const calls: string[] = [];
  const client = {
    getFileMetadata: async (path: string) => {
      calls.push(path);
      if (path.endsWith("broken.md")) throw new Error("403");
      return { name: path.split("/").pop()!, path, size: 2048, type: "text/markdown", last_modified: 1_760_000_000_000 };
    },
    listWorkspaceFiles: async () => [file("a.md"), file("broken.md")],
  };
  return { calls, client };
}

describe("appStore · loadArtifactMetadata（R20）", () => {
  beforeEach(() => {
    useAppStore.setState({
      client: null,
      artifactsRoot: null,
      artifacts: [],
      artifactsMeta: {},
    });
  });

  it("给列表里每个未记账的路径查一次元数据", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      artifactsRoot: ROOT,
      artifacts: [file("a.md"), file("b.md")],
    });
    await useAppStore.getState().loadArtifactMetadata();
    expect(fake.calls.sort()).toEqual([`${ROOT}/a.md`, `${ROOT}/b.md`]);
    const meta = useAppStore.getState().artifactsMeta;
    expect(meta[`${ROOT}/a.md`]).toMatchObject({ size: 2048, type: "text/markdown" });
  });

  it("失败记成 null（不是漏账），下一次调用不再重复发请求", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      artifactsRoot: ROOT,
      artifacts: [file("broken.md")],
    });
    await useAppStore.getState().loadArtifactMetadata();
    expect(useAppStore.getState().artifactsMeta[`${ROOT}/broken.md`]).toBeNull();
    expect(useAppStore.getState().artifactsError).toBeNull();

    await useAppStore.getState().loadArtifactMetadata();
    expect(fake.calls).toHaveLength(1);
  });

  it("显式传路径时只查这些路径", async () => {
    const fake = fakeClient();
    useAppStore.setState({
      client: fake.client as never,
      artifactsRoot: ROOT,
      artifacts: [file("a.md"), file("b.md")],
    });
    await useAppStore.getState().loadArtifactMetadata([`${ROOT}/b.md`]);
    expect(fake.calls).toEqual([`${ROOT}/b.md`]);
  });

  it("没有 client / 没有路径时是 no-op", async () => {
    await useAppStore.getState().loadArtifactMetadata();
    expect(useAppStore.getState().artifactsMeta).toEqual({});

    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT, artifacts: [] });
    await useAppStore.getState().loadArtifactMetadata();
    expect(fake.calls).toEqual([]);
  });

  it("没有选中工作区时刷新列表不查元数据（明确空态，不是静默失败）", async () => {
    const fake = fakeClient();
    useAppStore.setState({ client: fake.client as never, artifactsRoot: ROOT, artifacts: [file("a.md")] });
    await useAppStore.getState().refreshArtifacts();
    // 未选中会话 → 没有 workspace root：列表清空并且不发元数据请求。
    expect(useAppStore.getState().artifactsRoot).toBeNull();
    expect(useAppStore.getState().artifacts).toEqual([]);
    expect(fake.calls).toEqual([]);
  });
});
