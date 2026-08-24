/**
 * Regression tests for serialized canvas persistence flushes.
 *
 * Guards the durability contract of flushCanvasStorePersistence:
 * - flushes execute strictly sequentially (a slow IndexedDB write must never
 *   let a later flush land first and then be overwritten by the stale one);
 * - an explicit flush drains everything queued while it was waiting, so
 *   callers that report "saved" after awaiting it never lose edits.
 */
import { describe, expect, test } from "bun:test";

type BunMockModule = { module: (specifier: string, factory: () => unknown) => void };
// 静态导入的 bun:test 类型缺少 mock 成员（@types/bun 滞后），运行时存在；
// 经 unknown 中转做形状断言。
const bunMock = (await import("bun:test")) as unknown as typeof import("bun:test") & {
    mock: BunMockModule;
};

const backing = new Map<string, string>();

// 确定性门控：拦截下一次项目 blob 写入并挂起，由测试显式放行（不依赖真实时钟）。
const gate = { release: () => {} };
let armed = false;
bunMock.mock.module("@oc/lib/localforage-storage", () => ({
    localForageStorage: {
        getItem: async (name: string) => backing.get(name) ?? null,
        setItem: async (name: string, value: string) => {
            if (armed && name.startsWith("infinite-canvas:project:")) {
                armed = false;
                await new Promise<void>((resolve) => gate.release = resolve);
            }
            backing.set(name, value);
        },
        removeItem: async (name: string) => {
            backing.delete(name);
        },
    },
}));

type EnvelopeShape = { state: { projectIndex: Array<{ id: string; title?: string }> } };

// mock.module 必须先于被测模块导入注册，因此这里只能动态加载（模块装载边界测试）。
const { CANVAS_STORE_KEY } = await import("./canvas-store-persist");
const { flushCanvasStorePersistence, useCanvasStore } = await import("./use-canvas-store");

describe("useCanvasStore persistence", () => {
    test("serialized flushes converge to the newest payload even when a write stalls", async () => {
        const idA = useCanvasStore.getState().createProject("alpha");
        useCanvasStore.getState().createProject("beta");
        await flushCanvasStorePersistence();
        expect(backing.has(CANVAS_STORE_KEY)).toBe(true);

        // Arm the gate, let the flush reach it (one microtask hop onto the
        // persist chain), then land a newer edit while that write is held.
        armed = true;
        useCanvasStore.getState().renameProject(idA, "v2");
        const draining = flushCanvasStorePersistence();
        await new Promise<void>((resolve) => queueMicrotask(resolve));
        useCanvasStore.getState().renameProject(idA, "v3");
        gate.release();

        // The flush that started before "v3" was queued must still drain it
        // before resolving: returning early here would let callers report a
        // save that never happened (and lose "v3" if the app closed right now).
        await draining;

        const envelope = JSON.parse(backing.get(CANVAS_STORE_KEY)!) as EnvelopeShape;
        expect(envelope.state.projectIndex.find((entry) => entry.id === idA)?.title).toBe("v3");
        expect(JSON.parse(backing.get(`infinite-canvas:project:${idA}`)!).title).toBe("v3");
    });
});
