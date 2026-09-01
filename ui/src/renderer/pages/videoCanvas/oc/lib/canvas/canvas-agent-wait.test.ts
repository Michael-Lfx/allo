import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import { waitCanvasAgentGeneration } from "./canvas-agent-wait";

function snapshot(status: string): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [{ id: "img-1", type: CanvasNodeType.Image, title: "封面", position: { x: 0, y: 0 }, width: 200, height: 160, metadata: { status } as CanvasNodeData["metadata"] }],
        connections: [],
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("waitCanvasAgentGeneration", () => {
    test("returns after generating nodes reach a terminal status", async () => {
        let current = snapshot("running");
        let now = 0;
        const result = await waitCanvasAgentGeneration(
            () => current,
            { nodeIds: ["img-1"], timeoutMs: 10_000 },
            {
                now: () => now,
                sleep: async () => {
                    now += 1_500;
                    current = snapshot("success");
                },
            },
        );
        expect(result.timedOut).toBe(false);
        expect(result.pendingCount).toBe(0);
    });

    test("times out while nodes stay running", async () => {
        const result = await waitCanvasAgentGeneration(
            () => snapshot("running"),
            { nodeIds: ["img-1"], timeoutMs: 3_000 },
            {
                now: (() => {
                    let value = 0;
                    return () => {
                        const current = value;
                        value += 1_600;
                        return current;
                    };
                })(),
                sleep: async () => undefined,
            },
        );
        expect(result.timedOut).toBe(true);
        expect(result.pendingCount).toBeGreaterThan(0);
    });
});
