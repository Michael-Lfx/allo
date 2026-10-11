import { describe, expect, test } from "bun:test";

import { canvasLayerTransform } from "./canvas-live-viewport";

describe("canvasLayerTransform", () => {
    test("keeps the composited translate3d fast path by default", () => {
        expect(canvasLayerTransform(12, -8, 1.5)).toBe("translate3d(12px, -8px, 0) scale(1.5)");
    });

    test("uses a 2D transform for the Linux software-raster path", () => {
        const transform = canvasLayerTransform(12, -8, 0.75, true);
        expect(transform).toBe("translate(12px, -8px) scale(0.75)");
        expect(transform).not.toContain("translate3d");
    });
});
