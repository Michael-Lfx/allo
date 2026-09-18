import { describe, expect, test } from "bun:test";

import { getNodePanelPosition } from "./canvas-node-panel-position";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

function node(overrides: Partial<CanvasNodeData> = {}): CanvasNodeData {
    return {
        id: "node-1",
        type: CanvasNodeType.Image,
        title: "镜头",
        position: { x: 100, y: 80 },
        width: 200,
        height: 160,
        metadata: {},
        ...overrides,
    };
}

const identity = { x: 0, y: 0, k: 1 };
const viewportSize = { width: 800, height: 600 };
const panelWidth = 520;

describe("getNodePanelPosition", () => {
    test("anchors the edit panel below the node when there is room", () => {
        const position = getNodePanelPosition(node(), identity, viewportSize, panelWidth);
        expect(position.top).toBe(250);
        expect(position.left).toBe(12);
    });

    test("stays below the node near the canvas bottom instead of flipping above the toolbar", () => {
        const position = getNodePanelPosition(node({ position: { x: 100, y: 480 } }), identity, viewportSize, panelWidth);
        expect(position.top).toBe(650);
        expect(position.top).toBeGreaterThan(viewportSize.height);
    });

    test("does not pull the panel up to keep it fully on screen", () => {
        const position = getNodePanelPosition(node({ position: { x: 100, y: 360 } }), identity, viewportSize, panelWidth);
        expect(position.top).toBe(530);
    });
});
