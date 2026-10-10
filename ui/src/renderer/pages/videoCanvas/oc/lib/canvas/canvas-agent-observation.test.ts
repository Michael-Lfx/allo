import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import { applyCanvasAgentOps, type CanvasAgentSnapshot } from "./canvas-agent-ops";
import { buildCanvasAgentObservation, observationPromptBlock } from "./canvas-agent-observation";

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [{
            id: "img-1",
            type: CanvasNodeType.Image,
            title: "镜头1",
            position: { x: 0, y: 0 },
            width: 200,
            height: 160,
            metadata: { status: "pending", prompt: "猫" },
        }],
        connections: [],
        selectedNodeIds: ["img-1"],
        viewport: { x: 12, y: 8, k: 1.4 },
    };
}

describe("canvas agent observation", () => {
    test("marks a pending generation as incomplete queue, ignoring viewport in the fingerprint", () => {
        const current = applyCanvasAgentOps(snapshot(), []);
        const observation = buildCanvasAgentObservation(current);
        expect(observation.incomplete).toBe(false);
        expect(observation.queue[0]?.status).toBe("pending");
        expect(observation.selected.length).toBe(1);
        const panned = { ...current, viewport: { x: 99, y: 99, k: 2 }, selectedNodeIds: [] };
        expect(buildCanvasAgentObservation(panned).fingerprint).toBe(observation.fingerprint);
    });

    test("warns when live nodes overlap and forbids recreate", () => {
        const overlapping: CanvasAgentSnapshot = {
            ...snapshot(),
            nodes: [
                snapshot().nodes[0]!,
                { ...snapshot().nodes[0]!, id: "img-2", title: "镜头2", position: { x: 20, y: 20 } },
            ],
        };
        const text = observationPromptBlock(buildCanvasAgentObservation(overlapping));
        expect(text).toContain("组节点重叠");
        expect(text).toContain("禁止 deleteIds 后重建副本");
    });

    test("prompt block tells the model not to claim completion", () => {
        const text = observationPromptBlock(buildCanvasAgentObservation(snapshot()));
        expect(text).toContain("[画布观察]");
        expect(text).toContain("模板：无");
        expect(text).toContain("NEW：");
    });

    test("production goals are not incomplete while generation is still in the queue", () => {
        const boarded: CanvasAgentSnapshot = {
            ...snapshot(),
            nodes: [
                {
                    id: "script-1",
                    type: CanvasNodeType.Script,
                    title: "分镜",
                    position: { x: 0, y: 0 },
                    width: 280,
                    height: 200,
                    metadata: { storyboard: { rows: [{ id: "shot-1", plot: "大闹天宫开场" }] } },
                },
                {
                    id: "img-1",
                    type: CanvasNodeType.Image,
                    title: "孙悟空",
                    position: { x: 0, y: 240 },
                    width: 200,
                    height: 160,
                    metadata: { status: "loading", prompt: "齐天大圣金甲" },
                },
            ],
        };
        const observation = buildCanvasAgentObservation(boarded, null, { production: true, generation: false, layoutOnly: false });
        expect(observation.queue).toHaveLength(1);
        expect(observation.incomplete).toBe(false);
    });

    test("production goals stay incomplete while character images are idle", () => {
        const idle: CanvasAgentSnapshot = {
            ...snapshot(),
            nodes: [{
                id: "img-1",
                type: CanvasNodeType.Image,
                title: "孙悟空 · 角色圣经",
                position: { x: 0, y: 0 },
                width: 200,
                height: 160,
                metadata: { status: "idle", prompt: "齐天大圣金甲" },
            }],
        };
        const observation = buildCanvasAgentObservation(idle, null, { production: true, generation: false, layoutOnly: false });
        expect(observation.incomplete).toBe(true);
        expect(observation.idle).toHaveLength(1);
        expect(observationPromptBlock(observation)).toContain("仍 idle");
    });

    test("production goals stay incomplete when a storyboard has no film nodes", () => {
        const boarded: CanvasAgentSnapshot = {
            ...snapshot(),
            nodes: [{
                id: "script-1",
                type: CanvasNodeType.Script,
                title: "分镜",
                position: { x: 0, y: 0 },
                width: 280,
                height: 200,
                metadata: { storyboard: { rows: [{ id: "shot-1", plot: "大闹天宫开场" }] } },
            }],
        };
        const observation = buildCanvasAgentObservation(boarded, null, { production: true, generation: false, layoutOnly: false });
        expect(observation.incomplete).toBe(true);
        expect(observationPromptBlock(observation)).toContain("没有镜头视频");
    });

    test("diff lists NEW nodes against the previous snapshot", () => {
        const previous = applyCanvasAgentOps(snapshot(), []);
        const next = applyCanvasAgentOps(previous, [{ type: "add_node", id: "img-2", nodeType: CanvasNodeType.Image, title: "镜头2", position: { x: 240, y: 0 } }]);
        const observation = buildCanvasAgentObservation(next, previous);
        expect(observation.diff.new.some((item) => item.includes("镜头2"))).toBe(true);
    });
});
