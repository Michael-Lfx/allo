import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import {
    canvasAgentStateHashBlocksWrite,
    isGenerationOnlyCanvasAgentOps,
    verifyCanvasAgentOps,
    type CanvasAgentSnapshot,
} from "./canvas-agent-ops";

function snapshot(status: string, taskId?: string): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [{
            id: "img-1",
            type: CanvasNodeType.Image,
            title: "分镜1",
            position: { x: 0, y: 0 },
            width: 200,
            height: 160,
            metadata: { status, ...(taskId ? { taskId } : {}) } as CanvasNodeData["metadata"],
        }],
        connections: [],
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("verifyCanvasAgentOps generation", () => {
    test("treats pending nodes as queued even before a taskId is bound", () => {
        const result = verifyCanvasAgentOps(snapshot("idle"), snapshot("pending"), [{ type: "run_generation", nodeId: "img-1", mode: "image" }]);
        expect(result.ok).toBe(true);
        expect(result.generation).toEqual([expect.objectContaining({ nodeId: "img-1", outcome: "queued" })]);
    });

    test("still reports not_started when the node stays idle", () => {
        const result = verifyCanvasAgentOps(snapshot("idle"), snapshot("idle"), [{ type: "run_generation", nodeId: "img-1", mode: "image" }]);
        expect(result.ok).toBe(false);
        expect(result.generation[0]?.outcome).toBe("not_started");
    });

    test("treats overlapping new nodes as a warning, not a failed write", () => {
        const before: CanvasAgentSnapshot = { projectId: "p1", title: "画布", nodes: [], connections: [], selectedNodeIds: [], viewport: { x: 0, y: 0, k: 1 } };
        const after: CanvasAgentSnapshot = {
            ...before,
            nodes: [
                { id: "a", type: CanvasNodeType.Image, title: "A", position: { x: 0, y: 0 }, width: 200, height: 160, metadata: { status: "idle" } },
                { id: "b", type: CanvasNodeType.Image, title: "B", position: { x: 40, y: 40 }, width: 200, height: 160, metadata: { status: "idle" } },
            ],
        };
        const result = verifyCanvasAgentOps(before, after, [
            { type: "add_node", id: "a", nodeType: CanvasNodeType.Image },
            { type: "add_node", id: "b", nodeType: CanvasNodeType.Image },
        ]);
        expect(result.ok).toBe(true);
        expect(result.overlapWarnings.length).toBeGreaterThan(0);
    });
});

describe("canvasAgentStateHashBlocksWrite", () => {
    test("skips stale hashes for generation-only writes", () => {
        expect(isGenerationOnlyCanvasAgentOps([{ type: "run_generation", nodeId: "img-1", mode: "image" }])).toBe(true);
        expect(canvasAgentStateHashBlocksWrite("aaaa", "bbbb", "canvas_apply_ops", {
            ops: [{ type: "run_generation", nodeId: "img-1", mode: "image" }],
        })).toBe(false);
        expect(canvasAgentStateHashBlocksWrite("aaaa", "bbbb", "canvas_run", { nodeIds: ["img-1"] })).toBe(false);
        expect(canvasAgentStateHashBlocksWrite("aaaa", "bbbb", "canvas_apply_ops", {
            ops: [{ type: "add_node", id: "img-2", nodeType: CanvasNodeType.Image }],
        })).toBe(true);
    });
});
