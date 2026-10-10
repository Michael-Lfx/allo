import { describe, expect, test } from "bun:test";

import { defaultConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType } from "@oc/types/canvas";
import { CONTENT_MODERATION_ERROR_CODE, CONTENT_MODERATION_MESSAGE, generationPromptFingerprint } from "@oc/lib/generation-error";
import { APPLY_NEEDS_GRAPH_MESSAGE, CANVAS_AGENT_BUSY_RERUN_MESSAGE, CANVAS_AGENT_MODERATION_RERUN_MESSAGE, compileCanvasApplyOps, compileCanvasRepairOps, compileCanvasRunOps, critiqueCanvasOutputs, inspectCanvasIntent, proposeCanvasApply } from "./canvas-agent-intent";
import { APPLY_ALREADY_SATISFIED_MESSAGE } from "./canvas-agent-layout";
import { applyCanvasAgentOps } from "./canvas-agent-ops";
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

    test("refuses to rerun a node that is still generating or blocked by unchanged moderation", () => {
        const loading = {
            ...snapshot(),
            nodes: snapshot().nodes.map((node) => node.id === "vid-1" ? { ...node, metadata: { ...node.metadata, status: "loading" } } : node),
        };
        expect(() => compileCanvasRunOps(loading, ["vid-1"])).toThrow(CANVAS_AGENT_BUSY_RERUN_MESSAGE);
        const prompt = "齐天大圣金甲";
        const blocked = {
            ...snapshot(),
            nodes: [{
                id: "img-2",
                type: CanvasNodeType.Image,
                title: "角色",
                position: { x: 0, y: 0 },
                width: 200,
                height: 160,
                metadata: {
                    status: "error",
                    prompt,
                    generationErrorCode: CONTENT_MODERATION_ERROR_CODE,
                    errorDetails: CONTENT_MODERATION_MESSAGE,
                    failedPromptFingerprint: generationPromptFingerprint(prompt),
                },
            }],
        };
        expect(() => compileCanvasRunOps(blocked, ["img-2"])).toThrow(CANVAS_AGENT_MODERATION_RERUN_MESSAGE);
        expect(() => compileCanvasRepairOps({ action: "rerun", nodeIds: ["img-2"] }, blocked)).toThrow(CANVAS_AGENT_MODERATION_RERUN_MESSAGE);
        expect(() => compileCanvasRepairOps({ action: "rerun" }, blocked)).toThrow(CANVAS_AGENT_MODERATION_RERUN_MESSAGE);
    });

    test("inspect returns observation plus graph without requiring get_context", () => {
        const data = inspectCanvasIntent(snapshot(), {});
        expect(data.observation.nodeCount).toBe(2);
        expect(data.observation.incomplete).toBe(false);
        expect("graph" in data).toBe(true);
        expect("creation" in data).toBe(false);
    });

    test("inspect includes creation memory when sidecar exists", () => {
        const data = inspectCanvasIntent({
            ...snapshot(),
            alloCreative: {
                creation: {
                    schema: 1,
                    view: "storyboard",
                    spec: { aspectRatio: "16:9", resolution: "1080p", durationSecs: 6, mediaKind: "video" },
                    subjects: [{ id: "sub_1", kind: "character", name: "噜噜" }],
                    shots: [{ id: "s1", title: "1", plot: "出门", durationSecs: 6, subjectIds: ["sub_1"], status: "idle" }],
                },
            },
        }, {});
        expect("creation" in data).toBe(true);
        const creation = (data as { creation?: { subjects: Array<{ name: string }>; gaps: string[] } }).creation;
        expect(creation?.subjects[0]?.name).toBe("噜噜");
        expect(creation?.gaps.some((gap) => gap.includes("missing still"))).toBe(true);
    });

    test("inspect focus storyboard returns domain gaps without dumping the graph", () => {
        const data = inspectCanvasIntent({
            ...snapshot(),
            alloCreative: {
                creation: {
                    schema: 1,
                    view: "storyboard",
                    spec: { aspectRatio: "16:9", resolution: "1080p", durationSecs: 6, mediaKind: "video" },
                    subjects: [{ id: "sub_1", kind: "character", name: "噜噜" }],
                    shots: [{ id: "s1", title: "1", plot: "出门", durationSecs: 6, subjectIds: ["sub_1"], status: "idle" }],
                },
            },
        }, { focus: "storyboard" });
        expect("graph" in data).toBe(false);
        expect((data as { focus?: string }).focus).toBe("storyboard");
        const domain = (data as { domain?: { gaps?: string[] } }).domain;
        expect(domain?.gaps?.some((gap) => gap.includes("missing still"))).toBe(true);
    });

    test("inspect graph includes geometry so layout can patch positions", () => {
        const data = inspectCanvasIntent(snapshot(), {});
        const graph = (data as { graph?: { nodes: Array<{ x: number; y: number; width: number; height: number }> } }).graph;
        expect(graph?.nodes[0]?.x).toBe(0);
        expect(graph?.nodes[0]?.width).toBe(200);
    });

    test("hoists metadata.position onto node.position instead of burying geometry", () => {
        const ops = compileCanvasApplyOps({
            patches: [{ id: "img-1", metadata: { position: { x: 200, y: 100 } } }],
        }, snapshot(), defaultConfig);
        expect(ops).toEqual([
            expect.objectContaining({ type: "update_node", id: "img-1", patch: { position: { x: 200, y: 100 } } }),
        ]);
        const next = applyCanvasAgentOps(snapshot(), ops);
        expect(next.nodes.find((node) => node.id === "img-1")?.position).toEqual({ x: 200, y: 100 });
        expect(next.nodes.find((node) => node.id === "img-1")?.metadata?.position).toBeUndefined();
    });

    test("layout language plus recreate args moves existing nodes instead of cloning them", () => {
        const ops = compileCanvasApplyOps({
            description: "纵向重新排列，消除重叠",
            direction: "vertical",
            gap: 80,
            deleteIds: ["img-1", "vid-1"],
            nodes: [
                { ref: "n1", kind: "image", title: "镜头1", prompt: "猫出门" },
                { ref: "n2", kind: "video", title: "成片", prompt: "小猫的一天" },
            ],
            edges: [{ from: "n1", to: "n2" }],
        }, snapshot(), defaultConfig);
        expect(ops.some((op) => op.type === "add_node")).toBe(false);
        expect(ops.some((op) => op.type === "delete_node")).toBe(false);
        expect(ops.some((op) => op.type === "update_node" && op.id === "vid-1")).toBe(true);
        expect(ops.filter((op) => op.type === "update_node").every((op) => op.type === "update_node" && (op.id === "img-1" || op.id === "vid-1"))).toBe(true);
    });

    test("stale deleteIds plus a duplicate graph refuses to clone", () => {
        expect(() => compileCanvasApplyOps({
            description: "删除重叠节点后重建",
            deleteIds: ["missing-a", "missing-b"],
            nodes: [
                { ref: "n1", kind: "image", title: "海报", prompt: "新图" },
                { ref: "n2", kind: "video", title: "新片子", prompt: "新视频" },
            ],
        }, snapshot(), defaultConfig)).toThrow("拒绝再创建副本");
    });

    test("repeat position patches are already satisfied", () => {
        expect(() => compileCanvasApplyOps({
            patches: [{ id: "img-1", position: { x: 0, y: 0 } }],
        }, snapshot(), defaultConfig)).toThrow(APPLY_ALREADY_SATISFIED_MESSAGE);
    });
});
