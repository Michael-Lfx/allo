import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import { waitCanvasAgentGeneration, waitForInboundCanvasImages } from "./canvas-agent-wait";

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

function videoSnapshot(image: { status?: string; content?: string; prompt?: string }): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [
            { id: "img-1", type: CanvasNodeType.Image, title: "关键帧", position: { x: 0, y: 0 }, width: 200, height: 160, metadata: { status: image.status, content: image.content, prompt: image.prompt } as CanvasNodeData["metadata"] },
            { id: "video-1", type: CanvasNodeType.Video, title: "成片", position: { x: 400, y: 0 }, width: 480, height: 270 },
        ],
        connections: [{ id: "c1", fromNodeId: "img-1", toNodeId: "video-1" }],
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

    test("treats loading nodes as still generating", async () => {
        const result = await waitCanvasAgentGeneration(
            () => snapshot("loading"),
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

describe("waitForInboundCanvasImages", () => {
    test("waits for idle keyframe images that already have a prompt", async () => {
        let current = videoSnapshot({ status: "idle", prompt: "清晨小猫醒来" });
        let now = 0;
        const result = await waitForInboundCanvasImages(
            () => current,
            "video-1",
            {
                timeoutMs: 5_000,
                now: () => now,
                sleep: async () => {
                    now += 1_500;
                    current = videoSnapshot({ status: "success", content: "data:image/png;base64,a", prompt: "清晨小猫醒来" });
                },
            },
        );
        expect(result.timedOut).toBe(false);
        expect(result.ready).toBe(true);
        expect(result.imageIds).toEqual(["img-1"]);
    });

    test("does not wait for empty inbound images that have no prompt", async () => {
        const result = await waitForInboundCanvasImages(
            () => videoSnapshot({ status: "idle" }),
            "video-1",
            { timeoutMs: 1_000, now: () => 0, sleep: async () => undefined },
        );
        expect(result.timedOut).toBe(false);
        expect(result.ready).toBe(false);
        expect(result.imageIds).toEqual([]);
    });
});
