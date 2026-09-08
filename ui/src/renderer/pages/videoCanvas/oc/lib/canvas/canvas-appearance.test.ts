import { describe, expect, test } from "bun:test";

import {
    canvasAppearanceForTheme,
    customCanvasAppearanceFromTheme,
    DEFAULT_CANVAS_COLOR_THEME,
    enterCustomCanvasAppearance,
    normalizeCanvasAppearance,
    normalizeHexColor,
    resolveCanvasAppearance,
    resolveCanvasGridColor,
    resolveCanvasGridPalette,
    resolveStoredCanvasAppearance,
    scaleCssRgbaAlpha,
} from "./canvas-appearance";

describe("canvas custom appearance", () => {
    test("inherits the active fixed theme the first time custom mode is selected", () => {
        const light = enterCustomCanvasAppearance(canvasAppearanceForTheme("light"), "light");
        expect(light).toEqual({
            mode: "custom",
            custom: {
                baseTheme: "light",
                backgroundColor: "#EDEEEE",
                backgroundBrightness: 0,
                gridColor: "#B6B3B3",
                gridOpacity: 70,
            },
        });

        const dark = enterCustomCanvasAppearance(canvasAppearanceForTheme("dark"), "dark");
        expect(dark.custom).toMatchObject({ baseTheme: "dark", backgroundColor: "#090A0C", backgroundBrightness: 0, gridColor: "#B6B3B3", gridOpacity: 70 });
    });

    test("restores a previous custom profile only under the same base theme", () => {
        const previous = customCanvasAppearanceFromTheme("light");
        previous.custom = { ...previous.custom!, backgroundColor: "#F3DCE5" };
        const fixedLight = canvasAppearanceForTheme("light", previous);
        expect(enterCustomCanvasAppearance(fixedLight, "light").custom).toEqual(previous.custom);

        const fixedDark = canvasAppearanceForTheme("dark", previous);
        expect(fixedDark.custom).toBeUndefined();
        expect(enterCustomCanvasAppearance(fixedDark, "dark").custom).toMatchObject({
            baseTheme: "dark",
            backgroundColor: "#090A0C",
        });
    });

    test("normalizes hex colors", () => {
        expect(normalizeHexColor("F3DCE5")).toBe("#F3DCE5");
        expect(normalizeHexColor("#f3d")).toBe("#FF33DD");
    });

    test("adjusts only the custom canvas substrate and grid", () => {
        const appearance = normalizeCanvasAppearance({
            mode: "custom",
            custom: {
                baseTheme: "light",
                backgroundColor: "#F3DCE5",
                backgroundBrightness: 0,
                gridColor: "#9D7182",
                gridOpacity: 22,
            },
        }, "light");

        const resolved = resolveCanvasAppearance(appearance, "light");
        expect(resolved.baseTheme).toBe("light");
        expect(resolved.background).toBe("#F3DCE5");
        expect(resolveCanvasGridColor(appearance, "light", "lines")).toBe("rgba(157,113,130,0.22)");
        expect(scaleCssRgbaAlpha("rgba(157,113,130,0.22)", 0.5)).toBe("rgba(157,113,130,0.11)");
        expect(resolveCanvasGridPalette(appearance, "light", "lines")).toEqual({
            accent: "rgba(157,113,130,0.114)",
            muted: "rgba(157,113,130,0.048)",
        });
        expect(resolveCanvasGridPalette(appearance, "light", "dots").accent).toBe("rgba(157,113,130,0.084)");

        appearance.custom!.backgroundBrightness = 10;
        const brighter = resolveCanvasAppearance(appearance, "light").background;
        appearance.custom!.backgroundBrightness = -10;
        const darker = resolveCanvasAppearance(appearance, "light").background;

        const brighterParts = brighter.match(/^oklch\(([\d.]+)% ([\d.]+) ([\d.]+)\)$/);
        const darkerParts = darker.match(/^oklch\(([\d.]+)% ([\d.]+) ([\d.]+)\)$/);
        expect(brighterParts).not.toBeNull();
        expect(darkerParts).not.toBeNull();
        expect(Number(brighterParts![1])).toBeGreaterThan(Number(darkerParts![1]));
        expect(brighterParts!.slice(2)).toEqual(darkerParts!.slice(2));
    });

    test("missing appearance falls back to light, not dark", () => {
        expect(DEFAULT_CANVAS_COLOR_THEME).toBe("light");
        expect(normalizeCanvasAppearance(undefined, DEFAULT_CANVAS_COLOR_THEME)).toEqual({ mode: "light" });
        expect(resolveStoredCanvasAppearance({ mode: "dark" })).toEqual({ mode: "dark" });
        expect(resolveCanvasAppearance(undefined, DEFAULT_CANVAS_COLOR_THEME).baseTheme).toBe("light");
    });

    test("uses a quieter major/minor lattice for built-in themes", () => {
        const lightLines = resolveCanvasGridPalette({ mode: "light" }, "light", "lines");
        const lightDots = resolveCanvasGridPalette({ mode: "light" }, "light", "dots");
        expect(lightLines.accent).toBe("rgba(72,80,92,.11)");
        expect(lightLines.muted).toBe("rgba(72,80,92,.05)");
        expect(lightDots.accent).toBe("rgba(72,80,92,.18)");
        expect(lightDots.muted).toBe(lightDots.accent);
    });
});
