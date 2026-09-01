import { describe, expect, test } from "bun:test";
import i18n from "i18next";

import { defaultConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType } from "@oc/types/canvas";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "./canvas-agent-ops";
import { buildCanvasAgentPlan } from "./canvas-agent-plan";

i18n.init({ lng: "zh-CN", fallbackLng: "zh-CN", resources: { "zh-CN": { translation: {} } }, initImmediate: false });

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [{ id: "img-1", type: CanvasNodeType.Image, title: "封面", position: { x: 0, y: 0 }, width: 200, height: 160, metadata: { model: "flux-pro" } }],
        connections: [],
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("buildCanvasAgentPlan", () => {
    test("summarizes create, generate, and spend", () => {
        const ops: CanvasAgentOp[] = [
            { type: "add_node", id: "", nodeType: CanvasNodeType.Text, title: "提示" },
            { type: "run_generation", nodeId: "img-1", mode: "image" },
        ];
        const plan = buildCanvasAgentPlan(ops, snapshot(), defaultConfig);
        expect(plan.spend).toBe(true);
        expect(plan.generationCount).toBe(1);
        expect(plan.models).toContain("flux-pro");
        expect(plan.stages.some((stage) => stage.spend)).toBe(true);
        expect(plan.warning).toContain("费用");
    });

    test("does not mark spend for layout-only ops", () => {
        const plan = buildCanvasAgentPlan([{ type: "select_nodes", ids: ["img-1"] }], snapshot(), defaultConfig);
        expect(plan.spend).toBe(false);
        expect(plan.generationCount).toBe(0);
    });
});
