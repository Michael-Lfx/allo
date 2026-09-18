import { describe, expect, test } from "bun:test";

import { connectedNodeCenterFromEdgeDrop } from "./canvas-connected-node-placement";

describe("connected node placement", () => {
    test("places a new node to the right of a source drop", () => {
        expect(connectedNodeCenterFromEdgeDrop({ x: 100, y: 40 }, { width: 80, height: 60 }, "source")).toEqual({ x: 140, y: 40 });
    });

    test("places a new node to the left of a target drop", () => {
        expect(connectedNodeCenterFromEdgeDrop({ x: 100, y: 40 }, { width: 80, height: 60 }, "target")).toEqual({ x: 60, y: 40 });
    });
});
