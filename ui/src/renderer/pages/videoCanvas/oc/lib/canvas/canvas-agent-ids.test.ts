import { describe, expect, test } from "bun:test";

import { CanvasNodeType } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import {
    buildCanvasAgentAliasMap,
    canvasAgentNodeChangeKind,
    canvasAgentShortId,
    parseCanvasAgentMentionTokens,
    resolveCanvasAgentNodeId,
    resolveCanvasAgentNodeIds,
    stampCanvasAgentAliases,
} from "./canvas-agent-ids";

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "p1",
        title: "画布",
        nodes: [
            { id: "text-b", type: CanvasNodeType.Text, title: "B", position: { x: 0, y: 0 }, width: 100, height: 80 },
            { id: "text-a", type: CanvasNodeType.Text, title: "A", position: { x: 10, y: 0 }, width: 100, height: 80 },
        ],
        connections: [],
        selectedNodeIds: ["text-a"],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("canvas agent ids", () => {
    test("assigns stable short ids by sorted node id", () => {
        const aliases = buildCanvasAgentAliasMap(snapshot().nodes);
        expect(canvasAgentShortId("text-a", aliases)).toBe("n1");
        expect(canvasAgentShortId("text-b", aliases)).toBe("n2");
        expect(resolveCanvasAgentNodeId(snapshot(), "n2")).toBe("text-b");
        expect(resolveCanvasAgentNodeId(snapshot(), "text-a")).toBe("text-a");
    });

    test("keeps stamped aliases when a new node sorts earlier", () => {
        const stamped = stampCanvasAgentAliases(snapshot().nodes);
        expect(stamped.find((node) => node.id === "text-a")?.metadata?.agentAlias).toBe("n1");
        expect(stamped.find((node) => node.id === "text-b")?.metadata?.agentAlias).toBe("n2");
        const withNew = stampCanvasAgentAliases([
            { id: "aaa-new", type: CanvasNodeType.Image, title: "新图", position: { x: 0, y: 0 }, width: 100, height: 80, metadata: {} },
            ...stamped,
        ]);
        expect(withNew.find((node) => node.id === "text-a")?.metadata?.agentAlias).toBe("n1");
        expect(withNew.find((node) => node.id === "aaa-new")?.metadata?.agentAlias).toBe("n3");
        const aliases = buildCanvasAgentAliasMap(withNew);
        expect(canvasAgentShortId("text-a", aliases)).toBe("n1");
        expect(canvasAgentShortId("aaa-new", aliases)).toBe("n3");
    });

    test("parses @[node:id] and @n1 mention tokens", () => {
        expect(parseCanvasAgentMentionTokens("看 @[node:text-a] 和 @n2 一起改")).toEqual(["text-a", "n2"]);
        expect(resolveCanvasAgentNodeIds(snapshot(), ["n1", "missing"]).ids).toEqual(["text-a"]);
        expect(resolveCanvasAgentNodeIds(snapshot(), ["n1", "missing"]).missing).toEqual(["missing"]);
    });

    test("marks new and modified nodes", () => {
        const [node] = snapshot().nodes;
        expect(canvasAgentNodeChangeKind(undefined, node)).toBe("new");
        expect(canvasAgentNodeChangeKind(node, node)).toBeUndefined();
        expect(canvasAgentNodeChangeKind(node, { ...node, title: "改过" })).toBe("modified");
    });
});
