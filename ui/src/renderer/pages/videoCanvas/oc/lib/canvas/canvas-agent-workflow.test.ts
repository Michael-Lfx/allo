import { describe, expect, test } from "bun:test";

import { defaultConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import { partitionCanvasGenerationOps } from "./canvas-agent-ops";
import { buildCanvasWorkflowOps } from "./canvas-agent-workflow";

function snapshot(nodeId = "agent-workflow-n1-XKb4UFkM"): CanvasAgentSnapshot {
    return {
        projectId: "project-1",
        title: "测试画布",
        nodes: [
            { id: nodeId, type: CanvasNodeType.Image, title: "角色定妆", position: { x: 0, y: 0 }, width: 420, height: 300 },
        ],
        connections: [],
        selectedNodeIds: [nodeId],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("buildCanvasWorkflowOps", () => {
    test("connects edges from an existing canvas node into newly created refs", () => {
        const existingId = "agent-workflow-n1-XKb4UFkM";
        const ops = buildCanvasWorkflowOps({
            title: "关键帧 + 视频",
            nodes: [
                { ref: "frame1", kind: "image", title: "关键帧1", prompt: "晨起", referenceNodeIds: [existingId] },
                { ref: "finalVideo", kind: "video", title: "成片", prompt: "一天", runGeneration: false },
            ],
            edges: [
                { from: existingId, to: "frame1" },
                { from: "frame1", to: "finalVideo" },
            ],
        }, snapshot(existingId), defaultConfig);

        const connects = ops.filter((op) => op.type === "connect_nodes");
        const frame1Id = ops.find((op) => op.type === "add_node" && op.title === "关键帧1")?.id;
        const videoId = ops.find((op) => op.type === "add_node" && op.title === "成片")?.id;
        expect(frame1Id).toBeTruthy();
        expect(connects).toContainEqual({ type: "connect_nodes", fromNodeId: existingId, toNodeId: frame1Id });
        expect(connects).toContainEqual({ type: "connect_nodes", fromNodeId: frame1Id, toNodeId: videoId });
        expect(connects.filter((op) => op.fromNodeId === existingId && op.toNodeId === frame1Id)).toHaveLength(1);
    });

    test("resolves short ids in edges and still rejects unknown tokens", () => {
        const ops = buildCanvasWorkflowOps({
            nodes: [{ ref: "frame1", kind: "image", title: "关键帧1", prompt: "午后" }],
            edges: [{ from: "n1", to: "frame1" }],
        }, snapshot("existing-character"), defaultConfig);
        const frame1Id = ops.find((op) => op.type === "add_node")?.id;
        expect(ops).toContainEqual({ type: "connect_nodes", fromNodeId: "existing-character", toNodeId: frame1Id });

        expect(() => buildCanvasWorkflowOps({
            nodes: [{ ref: "frame1", kind: "image", title: "关键帧1", prompt: "午后" }],
            edges: [{ from: "missing-node", to: "frame1" }],
        }, snapshot(), defaultConfig)).toThrow("工作流连线引用不存在的节点：missing-node → frame1");
    });

    test("accepts same-batch refs inside referenceNodeIds (LLM mix-up)", () => {
        const existingId = "existing-character";
        const ops = buildCanvasWorkflowOps({
            description: "嘎子当兵",
            nodes: [
                {
                    ref: "img_keyframe",
                    kind: "image",
                    title: "关键帧：嘎子当兵",
                    prompt: "少年敬礼",
                    referenceNodeIds: ["n1"],
                },
                {
                    ref: "vid_gazi_salute",
                    kind: "video",
                    title: "视频：嘎子当兵",
                    prompt: "少年敬礼",
                    referenceNodeIds: ["img_keyframe"],
                },
            ],
        }, snapshot(existingId), defaultConfig);

        const imageId = ops.find((op) => op.type === "add_node" && op.title === "关键帧：嘎子当兵")?.id;
        const videoOp = ops.find((op) => op.type === "add_node" && op.title === "视频：嘎子当兵");
        const videoId = videoOp?.id;
        expect(imageId).toBeTruthy();
        expect(ops).toContainEqual({ type: "connect_nodes", fromNodeId: existingId, toNodeId: imageId });
        expect(ops).toContainEqual({ type: "connect_nodes", fromNodeId: imageId, toNodeId: videoId });
        expect(videoOp && "metadata" in videoOp ? videoOp.metadata?.videoStartFrameNodeId : undefined).toBe(imageId);
        expect(String(videoOp && "metadata" in videoOp ? videoOp.metadata?.prompt : "")).toContain(`@[node:${imageId}]`);

        expect(() => buildCanvasWorkflowOps({
            nodes: [{ ref: "frame1", kind: "image", title: "关键帧1", prompt: "午后", referenceNodeIds: ["missing-node"] }],
        }, snapshot(), defaultConfig)).toThrow("节点「关键帧1」引用的现有节点「missing-node」不存在");
    });

    test("fills storyboard rows, row handles, keyframe prompts, and video start/end frames", () => {
        const ops = buildCanvasWorkflowOps({
            title: "小猫的一天",
            autoRun: true,
            nodes: [
                { ref: "script", kind: "script", title: "分镜脚本", content: "场景1（5s）：清晨小猫醒来。\n场景2（5s）：正午窗台晒太阳。\n场景3（5s）：夜晚窝里睡觉。" },
                { ref: "f1", kind: "image", title: "关键帧1", prompt: "占位晨起" },
                { ref: "f2", kind: "image", title: "关键帧2", prompt: "占位正午" },
                { ref: "f3", kind: "image", title: "关键帧3", prompt: "占位夜晚" },
                { ref: "video", kind: "video", title: "成片", prompt: "小猫走过一天", seconds: "6" },
            ],
            edges: [
                { from: "script", to: "f1" },
                { from: "script", to: "f2" },
                { from: "script", to: "f3" },
                { from: "f1", to: "video" },
                { from: "f2", to: "video" },
                { from: "f3", to: "video" },
            ],
        }, snapshot(), defaultConfig);

        const scriptOp = ops.find((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Script);
        const rows = scriptOp && "metadata" in scriptOp ? scriptOp.metadata?.storyboard?.rows || [] : [];
        expect(rows).toHaveLength(3);
        expect(rows.map((row) => row.plotDescription).join(" ")).toContain("清晨小猫醒来");
        expect(rows.reduce((sum, row) => sum + row.durationSeconds, 0)).toBe(6);

        const imageOps = ops.filter((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Image);
        expect(imageOps.map((op) => (op.type === "add_node" ? op.metadata?.prompt : "")).join(" ")).toContain("清晨小猫醒来");
        expect(imageOps.map((op) => (op.type === "add_node" ? op.metadata?.prompt : "")).join(" ")).toContain("夜晚窝里睡觉");

        const rowConnects = ops.filter((op) => op.type === "connect_nodes" && op.fromHandleId?.startsWith("row:"));
        expect(rowConnects).toHaveLength(3);

        const videoOp = ops.find((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Video);
        const imageIds = imageOps.map((op) => (op.type === "add_node" ? op.id : "")).filter(Boolean);
        expect(videoOp && "metadata" in videoOp ? videoOp.metadata?.videoStartFrameNodeId : undefined).toBe(imageIds[0]);
        expect(videoOp && "metadata" in videoOp ? videoOp.metadata?.videoEndFrameNodeId : undefined).toBe(imageIds[2]);
        expect(videoOp && "metadata" in videoOp ? videoOp.metadata?.videoEditOperation : undefined).toBe("reference_to_video");
        expect(videoOp && "metadata" in videoOp ? videoOp.metadata?.seconds : undefined).toBe("6");
        const videoPrompt = videoOp && "metadata" in videoOp ? videoOp.metadata?.composerContent || "" : "";
        for (const imageId of imageIds) {
            expect(videoPrompt).toContain(`@[node:${imageId}]`);
        }

        const runModes = ops.filter((op) => op.type === "run_generation").map((op) => op.type === "run_generation" ? op.mode : undefined);
        expect(runModes).toEqual(["image", "image", "image", "video"]);
        const videoRun = ops.find((op) => op.type === "run_generation" && op.mode === "video");
        expect(videoRun && "prompt" in videoRun ? videoRun.prompt : "").toContain(`@[node:${imageIds[1]}]`);
    });

    test("stamps the home config card duration and ratio onto the video node", () => {
        const ops = buildCanvasWorkflowOps({
            nodes: [{ ref: "video", kind: "video", title: "成片", prompt: "一天", seconds: "15" }],
        }, {
            ...snapshot(),
            nodes: [{
                id: "cfg",
                type: CanvasNodeType.Config,
                title: "视频生成配置",
                position: { x: 0, y: 0 },
                width: 280,
                height: 180,
                metadata: { size: "16:9", seconds: "6", vquality: "720p", model: "allo-media::demo-video" },
            }],
        }, defaultConfig);
        const video = ops.find((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Video);
        expect(video && "metadata" in video ? video.metadata?.seconds : undefined).toBe("6");
        expect(video && "metadata" in video ? video.metadata?.size : undefined).toBe("16:9");
        expect(video && "metadata" in video ? video.metadata?.vquality : undefined).toBe("720p");
        expect(video && "metadata" in video ? video.metadata?.model : undefined).toBe("allo-media::demo-video");
    });
});

describe("partitionCanvasGenerationOps", () => {
    test("runs keyframe images before the video that depends on them", () => {
        const waves = partitionCanvasGenerationOps(
            [
                { type: "run_generation", nodeId: "video", mode: "video" },
                { type: "run_generation", nodeId: "f1", mode: "image" },
                { type: "run_generation", nodeId: "f2", mode: "image" },
            ],
            [
                { id: "c1", fromNodeId: "f1", toNodeId: "video" },
                { id: "c2", fromNodeId: "f2", toNodeId: "video" },
            ],
        );
        expect(waves).toHaveLength(2);
        expect(waves[0]?.map((op) => op.nodeId).sort()).toEqual(["f1", "f2"]);
        expect(waves[1]?.map((op) => op.nodeId)).toEqual(["video"]);
    });
});
