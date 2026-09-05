import { existsSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

import {
    compileCanvasStylePreset,
    lookbookCanvasStylePresets,
    parseCanvasStyleSelection,
    resolveCanvasStylePreset,
    styleCoverFromSelection,
} from "./canvas-style-system";

const INFRINGING_NEEDLES = [
    "邵氏",
    "新海诚",
    "胡金铨",
    "陈思诚",
    "HelloKitty",
    "Hello Kitty",
    "Miyazaki",
    "Pixar",
    "Shinkai",
    "short-drama-styles",
    ".jpg",
];

describe("canvas style system", () => {
    test("lookbook cards keep stable ids and compile from original combinator prompts", () => {
        const urban = resolveCanvasStylePreset("urban-live-action");
        expect(urban?.id).toBe("urban-live-action");
        expect(urban?.selection).toEqual({ world: "urban", tone: "romantic", medium: "live-action", character: "realistic" });
        expect(urban?.prompt).toContain("【题材世界观】");
        expect(urban?.prompt).toContain("当代中国都市");
        expect(urban?.cover.from).toMatch(/^#/);
        expect(lookbookCanvasStylePresets).toHaveLength(20);
    });

    test("existing black-white and dream ids map onto original tones instead of photo packs", () => {
        const noir = resolveCanvasStylePreset("black-white-noir");
        const dream = resolveCanvasStylePreset("surreal-dream");
        expect(noir?.selection?.tone).toBe("monochrome");
        expect(dream?.selection?.tone).toBe("oneiric");
        expect(noir?.cover).toEqual({ from: "#141414", via: "#7a7a7a", to: "#e8e8e8" });
    });

    test("lookbook copy stays original craft language", () => {
        const blob = lookbookCanvasStylePresets.map((preset) => `${preset.title}\n${preset.description}\n${preset.prompt}`).join("\n");
        for (const needle of INFRINGING_NEEDLES) {
            expect(blob.includes(needle)).toBe(false);
        }
    });

    test("custom mixes still resolve v2 ids with palette covers", () => {
        const id = "v2-wasteland--dark--live-action--realistic";
        expect(parseCanvasStyleSelection(id)).toEqual({ world: "wasteland", tone: "dark", medium: "live-action", character: "realistic" });
        const preset = compileCanvasStylePreset({ world: "wasteland", tone: "dark", medium: "live-action", character: "realistic" });
        expect(preset.id).toBe(id);
        expect(preset.cover).toEqual(styleCoverFromSelection(preset.selection!));
        expect(resolveCanvasStylePreset(id)?.id).toBe(id);
    });

    test("preview photo pack is gone from public assets", () => {
        expect(existsSync(join(process.cwd(), "public", "short-drama-styles"))).toBe(false);
    });
});
