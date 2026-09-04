import { describe, expect, test } from "bun:test";

import { defaultConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType } from "@oc/types/canvas";
import { APPLY_NEEDS_GRAPH_MESSAGE, compileCanvasApplyOps, compileCanvasRepairOps, compileCanvasRunOps, critiqueCanvasOutputs, inspectCanvasIntent, proposeCanvasApply } from "./canvas-agent-intent";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [
            {
                id: "img-1",
                type: CanvasNodeType.Image,
                title: "镜头1",
                position: { x: 0, y: 0 },
                width: 200,
                height: 160,
                metadata: { status: "success", prompt: "猫出门", storageKey: "resource:abc", primaryImageId: "img" },
            },
            {
                id: "vid-1",
                type: CanvasNodeType.Video,
                title: "成片",
                position: { x: 280, y: 0 },
                width: 280,
                height: 220,
                metadata: { status: "idle", prompt: "小猫的一天", composerContent: "小猫的一天" },
            },
        ],
        connections: [{ id: "c1", fromNodeId: "img-1", toNodeId: "vid-1" }],
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("canvas agent intent", () => {
    test("compileCanvasApplyOps invents a graph from nodes and edges without a fixed template", () => {
        const ops = compileCanvasApplyOps({
            nodes: [
                { ref: "script", kind: "script", title: "剧本", content: "第一镜：出门\n第二镜：午睡" },
                { ref: "shot1", kind: "image", title: "出门", prompt: "出门" },
                { ref: "shot2", kind: "image", title: "午睡", prompt: "午睡" },
                { ref: "video", kind: "video", title: "成片", prompt: "一天", seconds: "6" },
            ],
            edges: [
                { from: "script", to: "shot1" },
                { from: "script", to: "shot2" },
                { from: "shot1", to: "video" },
                { from: "shot2", to: "video" },
            ],
        }, snapshot(), defaultConfig);
        const video = ops.find((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Video);
        expect(ops.filter((op) => op.type === "add_node")).toHaveLength(4);
        expect(String(video && "metadata" in video ? video.metadata?.prompt : "")).toMatch(/@\[node:/);
        expect(ops.some((op) => op.type === "run_generation")).toBe(false);
    });

    test("propose is a dry run with spend stages and no write ops leaked as the only plan", () => {
        const proposed = proposeCanvasApply({
            nodes: [{ ref: "img", kind: "image", title: "海报", prompt: "一只猫" }],
            run: true,
        }, snapshot(), defaultConfig);
        expect(proposed.dryRun).toBe(true);
        expect(proposed.createdEstimate).toBe(1);
        expect(proposed.generationEstimate).toBe(1);
        expect(proposed.plan.spend).toBe(true);
    });

    test("critique flags a video that is missing inbound @ mentions", () => {
        const result = critiqueCanvasOutputs(snapshot(), ["vid-1"]);
        expect(result.ok).toBe(false);
        expect(result.issues[0]?.code).toBe("MISSING_REF");
        expect(result.issues[0]?.message).toContain("rewire_refs");
    });

    test("repair rewire_refs injects @ mentions and start/end frames", () => {
        const ops = compileCanvasRepairOps({ action: "rewire_refs", nodeIds: ["vid-1"] }, snapshot());
        expect(ops[0]).toMatchObject({ type: "update_node", id: "vid-1" });
        const metadata = ops[0] && ops[0].type === "update_node" ? ops[0].metadata : undefined;
        expect(String(metadata?.prompt || "")).toContain("@[node:img-1]");
        expect(metadata?.videoStartFrameNodeId).toBe("img-1");
        expect(metadata?.videoEndFrameNodeId).toBe("img-1");
    });

    test("empty apply with autoRun runs existing media instead of failing", () => {
        const ops = compileCanvasApplyOps({ autoRun: true, description: "小猫的一天" }, snapshot(), defaultConfig);
        expect(ops).toEqual([expect.objectContaining({ type: "run_generation", nodeId: "vid-1", mode: "video" })]);
    });

    test("empty apply on a canvas without runnable media asks for nodes", () => {
        const empty = { ...snapshot(), nodes: [], connections: [] };
        expect(() => compileCanvasApplyOps({ autoRun: true, description: "小猫的一天" }, empty, defaultConfig)).toThrow(APPLY_NEEDS_GRAPH_MESSAGE);
        expect(() => compileCanvasApplyOps({ description: "小猫的一天" }, snapshot(), defaultConfig)).toThrow(APPLY_NEEDS_GRAPH_MESSAGE);
    });

    test("run targets idle media with prompts and skips ready successes", () => {
        const ops = compileCanvasRunOps(snapshot());
        expect(ops).toEqual([expect.objectContaining({ type: "run_generation", nodeId: "vid-1", mode: "video" })]);
    });

    test("inspect returns observation plus graph without requiring get_context", () => {
        const data = inspectCanvasIntent(snapshot(), {});
        expect(data.observation.nodeCount).toBe(2);
        expect(data.observation.incomplete).toBe(false);
        expect("graph" in data).toBe(true);
    });
});
