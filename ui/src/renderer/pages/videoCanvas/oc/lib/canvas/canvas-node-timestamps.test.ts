import { describe, expect, test } from "bun:test";

import { normalizeCanvasNodeTimestamps, stampCanvasNodeChanges, updateCanvasNodes } from "./canvas-node-timestamps";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

function node(id: string, patch: Partial<CanvasNodeData> = {}): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Image,
        title: id,
        position: { x: 0, y: 0 },
        width: 320,
        height: 180,
        metadata: {},
        ...patch,
    };
}

describe("canvas node timestamps", () => {
    test("legacy nodes receive the project timestamp baseline", () => {
        const normalized = normalizeCanvasNodeTimestamps([node("legacy")], {
            createdAt: "2026-08-01T01:00:00.000Z",
            updatedAt: "2026-08-20T02:00:00.000Z",
        });
        expect(normalized[0]?.createdAt).toBe("2026-08-01T01:00:00.000Z");
        expect(normalized[0]?.updatedAt).toBe("2026-08-20T02:00:00.000Z");
    });

    test("new nodes and meaningful edits receive timestamps without touching unchanged nodes", () => {
        const created = stampCanvasNodeChanges([], [node("new")], "2026-08-28T01:00:00.000Z");
        expect(created[0]).toMatchObject({ createdAt: "2026-08-28T01:00:00.000Z", updatedAt: "2026-08-28T01:00:00.000Z" });

        const unchanged = created[0]!;
        const edited = { ...unchanged, title: "新标题" };
        const updated = stampCanvasNodeChanges(created, [edited], "2026-08-28T02:00:00.000Z");
        expect(updated[0]?.createdAt).toBe("2026-08-28T01:00:00.000Z");
        expect(updated[0]?.updatedAt).toBe("2026-08-28T02:00:00.000Z");
    });

    test("same-order identity reuse skips metadata comparison for untouched nodes", () => {
        const first = node("first", { createdAt: "2026-08-28T01:00:00.000Z", updatedAt: "2026-08-28T01:00:00.000Z" });
        const second = node("second", { createdAt: "2026-08-28T01:00:00.000Z", updatedAt: "2026-08-28T01:00:00.000Z" });
        const previous = [first, second];
        const next = stampCanvasNodeChanges(previous, [first, { ...second, title: "改过" }], "2026-08-28T02:00:00.000Z");
        expect(next[0]).toBe(first);
        expect(next[1]?.updatedAt).toBe("2026-08-28T02:00:00.000Z");
    });

    test("media hydration does not pretend to be a user edit", () => {
        const previous = node("image", {
            createdAt: "2026-08-28T01:00:00.000Z",
            updatedAt: "2026-08-28T01:00:00.000Z",
            metadata: { storageKey: "resource:1", content: "resource:1" },
        });
        const hydrated = { ...previous, metadata: { ...previous.metadata, content: "blob:preview", naturalWidth: 1920, naturalHeight: 1080, hasAudio: true, videoPreview: { content: "blob:preview" } } };
        const updated = stampCanvasNodeChanges([previous], [hydrated], "2026-08-28T02:00:00.000Z");
        expect(updated[0]?.updatedAt).toBe("2026-08-28T01:00:00.000Z");
    });

    test("batches media metadata updates while preserving untouched node references", () => {
        const first = node("first");
        const second = node("second");
        const nodes = [first, second];
        const next = updateCanvasNodes(nodes, new Map([
            ["first", (current) => ({ ...current, metadata: { ...current.metadata, naturalWidth: 1920 } })],
        ]), "2026-08-28T03:00:00.000Z");
        expect(next).not.toBe(nodes);
        expect(next[0]).not.toBe(first);
        expect(next[1]).toBe(second);
        expect(next[0].metadata?.naturalWidth).toBe(1920);
    });
});
