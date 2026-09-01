import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { rememberSnapshotNodes, rememberWriteTargets } from "./canvas-online-agent-loop";

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "project-1",
        title: "测试画布",
        nodes: [
            { id: "agent-workflow-keyframe-ZP80WC6f", type: CanvasNodeType.Image, title: "关键帧", position: { x: 0, y: 0 }, width: 420, height: 300 },
            { id: "agent-workflow-video-VYEhJ2MQ", type: CanvasNodeType.Video, title: "视频", position: { x: 500, y: 0 }, width: 280, height: 220 },
            { id: "video-1788241696743-h91hn", type: CanvasNodeType.Video, title: "成片", position: { x: 860, y: 0 }, width: 280, height: 220 },
        ],
        connections: [],
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("canvas agent write targeting", () => {
    test("snapshot nodes are treated as already known", () => {
        const inspected = new Set<string>();
        rememberSnapshotNodes(snapshot(), inspected);
        expect(inspected.has("agent-workflow-keyframe-ZP80WC6f")).toBe(true);
        expect(inspected.has("video-1788241696743-h91hn")).toBe(true);
    });

    test("updating existing workflow nodes does not require a prior canvas_get_node", () => {
        const inspected = new Set<string>();
        const created = new Set<string>();
        const error = rememberWriteTargets(
            "canvas_apply_ops",
            {
                ops: [
                    { type: "update_node", id: "agent-workflow-keyframe-ZP80WC6f", patch: { title: "关键帧" } },
                    { type: "update_node", id: "agent-workflow-video-VYEhJ2MQ", patch: { title: "视频" } },
                ],
            },
            snapshot(),
            inspected,
            created,
        );
        expect(error).toBe("");
        expect(inspected.has("agent-workflow-keyframe-ZP80WC6f")).toBe(true);
        expect(inspected.has("agent-workflow-video-VYEhJ2MQ")).toBe(true);
    });

    test("run_generation on an existing video node is allowed immediately", () => {
        const inspected = new Set<string>();
        const created = new Set<string>();
        expect(rememberWriteTargets("canvas_run_generation", { nodeId: "video-1788241696743-h91hn" }, snapshot(), inspected, created)).toBe("");
        expect(inspected.has("video-1788241696743-h91hn")).toBe(true);
    });
});
