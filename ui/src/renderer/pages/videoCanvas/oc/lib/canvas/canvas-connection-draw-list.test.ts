import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasDisplayConnection, type CanvasNodeData } from "@oc/types/canvas";
import {
    canvasCullViewRect,
    connectionTouchesView,
    applyDragPreviewToDisplayConnections,
    diffConnectionDrawList,
    filterDisplayConnections,
    type ViewRect,
} from "./canvas-connection-draw-list";

const VIEW: ViewRect = { left: 0, top: 0, right: 1000, bottom: 800 };

function node(id: string, x: number, y: number, width = 100, height = 80): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Text,
        title: id,
        position: { x, y },
        width,
        height,
    };
}

function display(id: string, from: CanvasNodeData, to: CanvasNodeData): CanvasDisplayConnection {
    return {
        connection: { id, fromNodeId: from.id, toNodeId: to.id },
        from,
        to,
    };
}

describe("canvas-connection-draw-list", () => {
    test("culls a connection when both endpoint bboxes sit fully outside the view", () => {
        const from = node("from", -400, -400);
        const to = node("to", 2000, 2000);
        const items = [display("edge-out", from, to)];

        expect(connectionTouchesView(from, to, VIEW)).toBe(false);
        expect(filterDisplayConnections(items, VIEW)).toEqual([]);
    });

    test("keeps a connection when one endpoint bbox intersects the view", () => {
        const from = node("from", 100, 100);
        const to = node("to", 2000, 2000);
        const items = [display("edge-partial", from, to)];

        expect(connectionTouchesView(from, to, VIEW)).toBe(true);
        expect(filterDisplayConnections(items, VIEW).map((item) => item.connection.id)).toEqual(["edge-partial"]);
    });

    test("diffs add and remove by connection id", () => {
        const keptFrom = node("kept-from", 10, 10);
        const keptTo = node("kept-to", 200, 10);
        const removed = display("edge-remove", node("old-from", 10, 200), node("old-to", 200, 200));
        const kept = display("edge-keep", keptFrom, keptTo);
        const added = display("edge-add", node("new-from", 10, 400), node("new-to", 200, 400));

        const { add, keep, remove } = diffConnectionDrawList(new Set([removed.connection.id, kept.connection.id]), [kept, added]);

        expect(add.map((item) => item.connection.id)).toEqual(["edge-add"]);
        expect(keep.map((item) => item.connection.id)).toEqual(["edge-keep"]);
        expect(remove).toEqual(["edge-remove"]);
    });

    test("keep includes the dragged edge when only endpoint geometry changed", () => {
        const to = node("to", 400, 40);
        const previous = display("edge-drag", node("from", 10, 40), to);
        const dragged = display("edge-drag", node("from", 80, 40), to);

        const { add, keep, remove } = diffConnectionDrawList(new Set([previous.connection.id]), [dragged]);

        expect(add).toHaveLength(0);
        expect(remove).toHaveLength(0);
        expect(keep).toHaveLength(1);
        expect(keep[0]?.connection.id).toBe("edge-drag");
        expect(keep[0]?.from.position.x).toBe(80);
    });

    test("offsets only display connections that touch the dragged node ids", () => {
        const from = node("from", 10, 40);
        const to = node("to", 400, 40);
        const otherFrom = node("other-from", 10, 400);
        const otherTo = node("other-to", 200, 400);
        const items = [display("edge-drag", from, to), display("edge-still", otherFrom, otherTo)];

        const next = applyDragPreviewToDisplayConnections(items, { x: 70, y: 5, nodeIds: ["from"] });

        expect(next[0]?.from.position).toEqual({ x: 80, y: 45 });
        expect(next[0]?.to.position).toEqual({ x: 400, y: 40 });
        expect(next[1]).toBe(items[1]);
    });

    test("cull padding matches node cull performance and quality multipliers", () => {
        const viewport = { x: 0, y: 0, k: 1 };
        const viewportSize = { width: 1000, height: 800 };
        const performancePad = Math.max(240, Math.max(1000, 800) * 0.4);
        const qualityPad = Math.max(800, Math.max(1000, 800) * 1.5);

        const performanceView = canvasCullViewRect(viewport, viewportSize, true);
        const qualityView = canvasCullViewRect(viewport, viewportSize, false);

        expect(performanceView).toEqual({
            left: -performancePad,
            top: -performancePad,
            right: 1000 + performancePad,
            bottom: 800 + performancePad,
        });
        expect(qualityView).toEqual({
            left: -qualityPad,
            top: -qualityPad,
            right: 1000 + qualityPad,
            bottom: 800 + qualityPad,
        });
    });
});
