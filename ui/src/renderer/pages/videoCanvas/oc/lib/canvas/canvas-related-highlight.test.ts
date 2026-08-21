import { describe, expect, test } from "bun:test";

import { canvasActiveNodeId, canvasRelatedHighlight } from "./canvas-related-highlight";

describe("canvas related highlight", () => {
    test("uses hover over a single selection, and ignores hover during multi-select", () => {
        expect(canvasActiveNodeId("hover", new Set(["selected"]))).toBe("hover");
        expect(canvasActiveNodeId(null, new Set(["selected"]))).toBe("selected");
        expect(canvasActiveNodeId("hover", new Set(["a", "b"]))).toBeNull();
        expect(canvasActiveNodeId(null, new Set())).toBeNull();
    });

    test("highlights the active node and both ends of its connections", () => {
        const highlight = canvasRelatedHighlight("a", [
            { id: "c1", fromNodeId: "a", toNodeId: "b" },
            { id: "c2", fromNodeId: "x", toNodeId: "y" },
        ]);

        expect([...highlight.nodeIds].sort()).toEqual(["a", "b"]);
        expect([...highlight.connectionIds]).toEqual(["c1"]);
    });
});
