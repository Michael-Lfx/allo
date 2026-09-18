import { describe, expect, test } from "bun:test";

import { contentSizeShouldNotify, resizeObserverWidthChanged } from "./canvas-content-size";

describe("canvas mention textarea size reporting", () => {
    test("does not notify parent when measured height only jitters", () => {
        expect(contentSizeShouldNotify(null, 80)).toBe(true);
        expect(contentSizeShouldNotify(80, 80)).toBe(false);
        expect(contentSizeShouldNotify(80, 80.4)).toBe(false);
        expect(contentSizeShouldNotify(80, 96)).toBe(true);
        expect(contentSizeShouldNotify(80, Number.NaN)).toBe(false);
    });

    test("ignores height-only resize observer callbacks", () => {
        expect(resizeObserverWidthChanged(null, 320)).toBe(true);
        expect(resizeObserverWidthChanged(320, 320)).toBe(false);
        expect(resizeObserverWidthChanged(320, 320.4)).toBe(false);
        expect(resizeObserverWidthChanged(320, 280)).toBe(true);
    });
});
