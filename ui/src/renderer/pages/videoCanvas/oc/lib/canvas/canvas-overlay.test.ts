import { describe, expect, test } from "bun:test";

import { anchoredOverlayStyle } from "./canvas-overlay";

const button = { left: 40, right: 160, top: 80, bottom: 108, width: 120 };
const viewport = { width: 1200, height: 800 };

describe("anchoredOverlayStyle", () => {
    test("clamps a left-aligned panel inside the viewport", () => {
        const style = anchoredOverlayStyle(button, viewport, { width: 440, placement: "bottomLeft" });
        expect(style.left).toBe(40);
        expect(style.top).toBe(116);
        expect(style.width).toBe(440);
    });

    test("flips above when there is not enough room below", () => {
        const low = { ...button, top: 740, bottom: 768 };
        const style = anchoredOverlayStyle(low, viewport, { width: 320, placement: "bottomLeft", estimatedHeight: 400 });
        expect(style.bottom).toBe(68);
        expect(style.top).toBeUndefined();
    });

    test("right-aligns to the trigger", () => {
        const right = { left: 820, right: 940, top: 80, bottom: 108, width: 120 };
        const style = anchoredOverlayStyle(right, viewport, { width: 200, placement: "topRight" });
        expect(style.left).toBe(right.right - 200);
    });

    test("keeps a tall audio settings card below the composer footer when there is room", () => {
        const style = anchoredOverlayStyle(button, viewport, { width: 356, placement: "bottomLeft", estimatedHeight: 520 });
        expect(style.top).toBe(116);
        expect(style.bottom).toBeUndefined();
        expect(style.zIndex).toBe("var(--z-popover)");
    });
});
