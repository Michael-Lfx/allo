import { describe, expect, test } from "bun:test";

import { CANVAS_NODE_TOOLBAR_GAP_PX, computeCanvasNodeToolbarAnchor } from "./canvas-node-toolbar-anchor";

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
});
