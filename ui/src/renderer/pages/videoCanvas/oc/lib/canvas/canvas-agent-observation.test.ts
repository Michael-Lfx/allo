import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import { applyCanvasAgentOps, type CanvasAgentSnapshot } from "./canvas-agent-ops";
import { buildCanvasAgentObservation, CANVAS_AGENT_CODES, observationPromptBlock } from "./canvas-agent-observation";

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
        expect(observation.incomplete).toBe(true);
        expect(observation.queue[0]?.status).toBe("pending");
        expect(observation.selected.length).toBe(1);
        const panned = { ...current, viewport: { x: 99, y: 99, k: 2 }, selectedNodeIds: [] };
        expect(buildCanvasAgentObservation(panned).fingerprint).toBe(observation.fingerprint);
    });

    test("prompt block tells the model not to claim completion", () => {
        const text = observationPromptBlock(buildCanvasAgentObservation(snapshot()));
        expect(text).toContain("[画布观察]");
        expect(text).toContain(CANVAS_AGENT_CODES.GOAL_INCOMPLETE);
        expect(text).toContain("NEW：");
    });

    test("diff lists NEW nodes against the previous snapshot", () => {
        const previous = applyCanvasAgentOps(snapshot(), []);
        const next = applyCanvasAgentOps(previous, [{ type: "add_node", id: "img-2", nodeType: CanvasNodeType.Image, title: "镜头2", position: { x: 240, y: 0 } }]);
        const observation = buildCanvasAgentObservation(next, previous);
        expect(observation.diff.new.some((item) => item.includes("镜头2"))).toBe(true);
    });
});
