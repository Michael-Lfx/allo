import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import {
    compileCanvasLayoutOps,
    looksLikeCanvasLayoutRequest,
    matchExistingNodesForApply,
    packCanvasNodes,
    shouldCompileCanvasApplyAsLayout,
} from "./canvas-agent-layout";

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [
            { id: "text-1", type: CanvasNodeType.Text, title: "角色描述", position: { x: 40, y: 40 }, width: 340, height: 240, metadata: { content: "角色身份" } },
            { id: "image-1", type: CanvasNodeType.Image, title: "设定图", position: { x: 48, y: 48 }, width: 420, height: 300, metadata: { prompt: "设定图" } },
            { id: "agent-workflow-n2-aaaa", type: CanvasNodeType.Image, title: "设定图", position: { x: 48, y: 48 }, width: 420, height: 300, metadata: { prompt: "设定图" } },
            { id: "image-2", type: CanvasNodeType.Image, title: "三视图", position: { x: 52, y: 52 }, width: 420, height: 300, metadata: { prompt: "三视图" } },
        ],
        connections: [{ id: "c1", fromNodeId: "text-1", toNodeId: "image-1" }],
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("canvas agent layout", () => {
    test("recognizes overlap cleanup language", () => {
        expect(looksLikeCanvasLayoutRequest("整理画布，让节点不在重叠")).toBe(true);
        expect(looksLikeCanvasLayoutRequest("小猫的一天")).toBe(false);
    });

    test("matches existing nodes by title and prefers originals over workflow clones", () => {
        const matched = matchExistingNodesForApply(snapshot(), [
            { ref: "n4", kind: "text", title: "角色描述" },
            { ref: "n2", kind: "image", title: "设定图" },
            { ref: "n3", kind: "image", title: "三视图" },
        ], [], "消除重叠");
        expect(matched?.map((node) => node.id)).toEqual(["text-1", "image-1", "image-2"]);
    });

    test("packs vertically with gap using live node sizes", () => {
        const nodes = snapshot().nodes.slice(0, 2);
        expect(packCanvasNodes(nodes, "vertical", 80, { x: 200, y: 100 })).toEqual([
            { x: 200, y: 100 },
            { x: 200, y: 100 + 240 + 80 },
        ]);
    });

    test("compile moves originals, deletes workflow clones, and does not add nodes", () => {
        const ops = compileCanvasLayoutOps({
            description: "纵向重新排列四个节点，消除重叠",
            direction: "vertical",
            gap: 80,
            start: { x: 200, y: 100 },
            deleteIds: ["text-1", "image-1", "image-2"],
            nodes: [
                { ref: "n4", kind: "text", title: "角色描述", content: "角色身份" },
                { ref: "n2", kind: "image", title: "设定图", prompt: "设定图" },
                { ref: "n3", kind: "image", title: "三视图", prompt: "三视图" },
            ],
            edges: [{ from: "n4", to: "n2" }, { from: "n2", to: "n3" }],
        }, snapshot());
        expect(ops.some((op) => op.type === "add_node")).toBe(false);
        expect(ops).toContainEqual({ type: "delete_node", ids: ["agent-workflow-n2-aaaa"] });
        expect(ops.filter((op) => op.type === "update_node").map((op) => op.type === "update_node" ? op.id : "")).toEqual(["text-1", "image-1", "image-2"]);
        expect(ops).toContainEqual({ type: "connect_nodes", fromNodeId: "image-1", toNodeId: "image-2" });
    });

    test("treats layout-language recreate as a layout compile", () => {
        expect(shouldCompileCanvasApplyAsLayout({
            description: "删除旧重叠节点，重建为纵向排列",
            deleteIds: ["text-1"],
            nodes: [{ ref: "n4", kind: "text", title: "角色描述" }],
        }, snapshot())).toBe(true);
    });
});
