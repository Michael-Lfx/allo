import { describe, expect, test } from "bun:test";

import {
    CANVAS_PERSIST_FORMAT,
    CANVAS_STORE_KEY,
    canvasProjectPersistKey,
    loadPersistedCanvasProjects,
    writeSplitCanvasProjects,
} from "./canvas-store-persist";

type TestProject = {
    id: string;
    title: string;
    createdAt: string;
    updatedAt: string;
    nodes: Array<{ id: string; label: string }>;
};

function memoryStorage() {
    const memory = new Map<string, string>();
    const setItemCalls: Array<{ name: string; value: string }> = [];
    return {
        memory,
        setItemCalls,
        storage: {
            getItem: async (name: string) => memory.get(name) ?? null,
            setItem: async (name: string, value: string) => {
                setItemCalls.push({ name, value });
                memory.set(name, value);
            },
            removeItem: async (name: string) => {
                memory.delete(name);
            },
        },
    };
}

function project(id: string, label: string): TestProject {
    return {
        id,
        title: `Canvas ${id}`,
        createdAt: "2024-01-01T00:00:00.000Z",
        updatedAt: "2024-01-02T00:00:00.000Z",
        nodes: [{ id: `node-${id}`, label }],
    };
}

describe("canvas store split persist", () => {
    test("hydrates the legacy single-blob format into split keys", async () => {
        const { memory, storage, setItemCalls } = memoryStorage();
        const projectA = project("a", "alpha-nodes");
        const projectB = project("b", "beta-nodes");
        memory.set(
            CANVAS_STORE_KEY,
            JSON.stringify({
                state: { projects: [projectA, projectB] },
                version: 0,
            }),
        );

        const loaded = await loadPersistedCanvasProjects<TestProject>(storage, CANVAS_STORE_KEY);

        expect(loaded?.state.projects.map((item) => item.id)).toEqual(["a", "b"]);
        expect(loaded?.state.projects[0]?.nodes[0]?.label).toBe("alpha-nodes");
        expect(JSON.parse(memory.get(canvasProjectPersistKey("a")) || "{}").nodes[0].label).toBe("alpha-nodes");
        expect(JSON.parse(memory.get(canvasProjectPersistKey("b")) || "{}").nodes[0].label).toBe("beta-nodes");

        const index = JSON.parse(memory.get(CANVAS_STORE_KEY) || "{}") as {
            persistFormat: number;
            state: { projectIndex: Array<{ id: string; title: string }>; projects?: unknown };
        };
        expect(index.persistFormat).toBe(CANVAS_PERSIST_FORMAT);
        expect(index.state.projects).toBeUndefined();
        expect(index.state.projectIndex.map((item) => item.id)).toEqual(["a", "b"]);
        expect(setItemCalls.map((call) => call.name)).toEqual([
            canvasProjectPersistKey("a"),
            canvasProjectPersistKey("b"),
            CANVAS_STORE_KEY,
        ]);
        expect("nodes" in (index.state.projectIndex[0] as object)).toBe(false);
    });

    test("editing project A does not rewrite project B's blob", async () => {
        const { storage, setItemCalls } = memoryStorage();
        const projectA = project("a", "alpha");
        const projectB = project("b", "beta");

        await writeSplitCanvasProjects(storage, CANVAS_STORE_KEY, { state: { projects: [projectA, projectB] }, version: 0 }, null);
        const blobBWritesAfterCreate = setItemCalls.filter((call) => call.name === canvasProjectPersistKey("b")).length;
        expect(blobBWritesAfterCreate).toBe(1);

        const editedA: TestProject = {
            ...projectA,
            updatedAt: "2024-06-01T00:00:00.000Z",
            nodes: [{ id: "node-a", label: "alpha-edited" }],
        };
        await writeSplitCanvasProjects(
            storage,
            CANVAS_STORE_KEY,
            { state: { projects: [editedA, projectB] }, version: 0 },
            [projectA, projectB],
        );

        expect(setItemCalls.filter((call) => call.name === canvasProjectPersistKey("b"))).toHaveLength(1);
        expect(setItemCalls.filter((call) => call.name === canvasProjectPersistKey("a"))).toHaveLength(2);
        expect(JSON.parse(setItemCalls.at(-1)?.value || "{}").state.projectIndex).toEqual([
            { id: "a", title: "Canvas a", createdAt: "2024-01-01T00:00:00.000Z", updatedAt: "2024-06-01T00:00:00.000Z" },
            { id: "b", title: "Canvas b", createdAt: "2024-01-01T00:00:00.000Z", updatedAt: "2024-01-02T00:00:00.000Z" },
        ]);
    });
});
