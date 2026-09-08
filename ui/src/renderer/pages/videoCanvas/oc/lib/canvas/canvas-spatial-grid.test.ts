import { describe, expect, test } from "bun:test";

import {
    CANVAS_DOT_MIN_SCREEN_STEP,
    CANVAS_GRID_MAJOR_EVERY,
    CANVAS_GRID_MIN_SCREEN_STEP,
    CANVAS_GRID_WORLD_STEP,
    canvasDotCoreRadiusPx,
    canvasDotFeatherRadiusPx,
    canvasDotGridSizePx,
    canvasDotsBackgroundImage,
    canvasGridBackgroundSize,
    canvasGridHairlinePx,
    canvasGridMajorSize,
    canvasGridScreenStep,
    canvasLinesBackgroundImage,
    canvasSpatialGridCssVars,
} from "./canvas-spatial-grid";

describe("canvas spatial grid", () => {
    test("keeps world-space 48px cells at typical zoom", () => {
        expect(canvasGridScreenStep(1)).toBe(CANVAS_GRID_WORLD_STEP);
        expect(canvasDotGridSizePx(1)).toBe(CANVAS_GRID_WORLD_STEP);
        expect(canvasGridMajorSize(48)).toBe(48 * CANVAS_GRID_MAJOR_EVERY);
    });

    test("floors screen spacing so far zoom does not collapse into gray mush", () => {
        expect(canvasGridScreenStep(0.05)).toBe(CANVAS_GRID_MIN_SCREEN_STEP);
        expect(canvasDotGridSizePx(0.05)).toBe(CANVAS_DOT_MIN_SCREEN_STEP);
        expect(canvasGridScreenStep(0.05)).toBeGreaterThan(48 * 0.05);
    });

    test("uses a device-pixel hairline that never goes below half a CSS pixel", () => {
        expect(canvasGridHairlinePx(1)).toBe(1);
        expect(canvasGridHairlinePx(2)).toBe(0.5);
        expect(canvasGridHairlinePx(3)).toBe(0.5);
    });

    test("softens dots with a feather larger than the core", () => {
        expect(canvasDotFeatherRadiusPx(1)).toBeGreaterThan(canvasDotCoreRadiusPx(1));
        expect(canvasDotCoreRadiusPx(0.1)).toBeLessThan(canvasDotCoreRadiusPx(1.4));
        expect(canvasDotsBackgroundImage("rgba(72,80,92,.18)")).toContain("transparent var(--canvas-dot-feather)");
        expect(canvasDotsBackgroundImage("rgba(72,80,92,.18)")).not.toContain("0.2px");
    });

    test("draws a major lattice every four minor cells and keeps both layers aligned", () => {
        const vars = canvasSpatialGridCssVars({ x: 50, y: -10, k: 1 }, 2);
        expect(vars["--canvas-grid-size"]).toBe("48px");
        expect(vars["--canvas-grid-major-size"]).toBe("192px");
        expect(vars["--canvas-grid-x"]).toBe("50px");
        expect(vars["--canvas-grid-y"]).toBe(`${-10 % 192}px`);
        expect(vars["--canvas-grid-hairline"]).toBe("0.5px");
        expect(canvasGridBackgroundSize("lines")).toContain("var(--canvas-grid-major-size)");
        const lines = canvasLinesBackgroundImage("rgba(72,80,92,.05)", "rgba(72,80,92,.11)");
        expect(lines.match(/linear-gradient/g)?.length).toBe(4);
        expect(lines).toContain("rgba(72,80,92,.11)");
        expect(lines).toContain("rgba(72,80,92,.05)");
    });
});
