import { describe, expect, test } from "bun:test";

import { CANVAS_NODE_TOOLBAR_GAP_PX, canvasNodeFallbackScreenRect, computeCanvasNodeToolbarAnchor, readCanvasNodeToolbarAnchor } from "./canvas-node-toolbar-anchor";

describe("computeCanvasNodeToolbarAnchor", () => {
    test("sits on the node card top in viewport space", () => {
        const anchor = computeCanvasNodeToolbarAnchor({
            nodeRect: { left: 400, top: 240, width: 200 },
            containerRect: { left: 0, top: 0, width: 1200, right: 1200 },
            toolbarWidth: 280,
        });
        expect(anchor.top).toBe(240 - CANVAS_NODE_TOOLBAR_GAP_PX);
        expect(anchor.left).toBe(500);
    });

    test("does not reserve space for the external title", () => {
        const anchor = computeCanvasNodeToolbarAnchor({
            nodeRect: { left: 100, top: 180, width: 120 },
            containerRect: { left: 40, top: 80, width: 800, right: 840 },
            toolbarWidth: 0,
        });
        expect(anchor.top).toBe(174);
        expect(anchor.left).toBe(160);
    });

    test("falls back to world position when the node element is missing", () => {
        expect(canvasNodeFallbackScreenRect({
            node: { position: { x: 80, y: 40 }, width: 200 },
            viewport: { x: 10, y: 20, k: 2 },
            containerRect: { left: 100, top: 50 },
        })).toEqual({ left: 270, top: 150, width: 400 });
    });

    test("reads the fallback rect when the node element is not in the container", () => {
        const container = {
            getBoundingClientRect: () => ({ left: 100, top: 50, width: 800, right: 900 }),
            querySelector: () => null,
        } as unknown as HTMLElement;
        expect(readCanvasNodeToolbarAnchor({
            node: { id: "n1", position: { x: 80, y: 40 }, width: 200 },
            viewport: { x: 10, y: 20, k: 2 },
            container,
            toolbarWidth: 0,
        })).toEqual({ left: 470, top: 150 - CANVAS_NODE_TOOLBAR_GAP_PX });
    });
});
