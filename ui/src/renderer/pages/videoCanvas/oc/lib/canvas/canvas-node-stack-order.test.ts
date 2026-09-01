import { describe, expect, test } from "bun:test";

import { bringCanvasNodeToFront, sortCanvasNodesByStackOrder } from "./canvas-node-stack-order";

describe("canvas node stack order", () => {
    test("moves the interacted node to the paint-order end", () => {
        expect(bringCanvasNodeToFront(["a", "b"], "a")).toEqual(["b", "a"]);
        expect(bringCanvasNodeToFront(["a", "b"], "c")).toEqual(["a", "b", "c"]);
        const alreadyFront = ["a", "b"];
        expect(bringCanvasNodeToFront(alreadyFront, "b")).toBe(alreadyFront);
    });

    test("keeps untouched nodes stable while the latest node remains on top", () => {
        const nodes = [{ id: "a" }, { id: "b" }, { id: "c" }];
        expect(sortCanvasNodesByStackOrder(nodes, ["b", "a"])).toEqual([{ id: "c" }, { id: "b" }, { id: "a" }]);
    });
});
