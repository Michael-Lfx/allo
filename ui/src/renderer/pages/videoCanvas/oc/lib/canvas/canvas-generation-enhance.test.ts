import { describe, expect, test } from "bun:test";

import { CANVAS_IMAGE_BATCH_MAX_COUNT, getCanvasBatchCount } from "./canvas-generation-count";
import {
    canvasGenerationSeed,
    detectCanvasGenerationIntent,
    enhanceCanvasGenerationPrompt,
    isCanvasPromptOptimizeEnabled,
    polishVideoPrompt,
    varyCanvasGenerationPrompt,
} from "./canvas-generation-enhance";

describe("canvas batch count", () => {
    test("clamps canvas fan-out independently of provider n=1", () => {
        expect(getCanvasBatchCount("4", CANVAS_IMAGE_BATCH_MAX_COUNT)).toBe(4);
        expect(getCanvasBatchCount("1", CANVAS_IMAGE_BATCH_MAX_COUNT)).toBe(1);
        expect(getCanvasBatchCount("99", CANVAS_IMAGE_BATCH_MAX_COUNT)).toBe(8);
        expect(getCanvasBatchCount(undefined, 4)).toBe(1);
    });
});

describe("canvas generation intent", () => {
    test("reads a short Chinese concept-sheet prompt as location + wasteland + white bg", () => {
        const intent = detectCanvasGenerationIntent("废土风 山姆超市 设定图 纯白背景");
        expect(intent.job).toBe("location-concept");
        expect(intent.whiteBackground).toBe(true);
        expect(intent.styleWorldId).toBe("wasteland");
    });

    test("character sheet keywords beat generic 设定图", () => {
        expect(detectCanvasGenerationIntent("角色设定图 正面侧面").job).toBe("character-sheet");
        expect(detectCanvasGenerationIntent("三视图 白底").job).toBe("turnaround");
    });
});

describe("canvas generation enhance", () => {
    test("injects location concept, white bg, and wasteland style for the supermarket brief", () => {
        const { prompt, intent } = enhanceCanvasGenerationPrompt("废土风 山姆超市 设定图 纯白背景");
        expect(intent.job).toBe("location-concept");
        expect(prompt).toContain("山姆超市");
        expect(prompt).toContain("【作业】场景设定图");
        expect(prompt).toContain("纯白无缝白底");
        expect(prompt).toContain("【视觉风格】末日废土");
        expect(prompt).toContain("尘土");
    });

    test("does not duplicate job or style when recipes or style packs are already present", () => {
        const withRecipe = enhanceCanvasGenerationPrompt("生成角色设定图：同一人正侧背");
        expect(withRecipe.prompt.match(/生成角色设定图/g)?.length).toBe(1);
        const withStyle = enhanceCanvasGenerationPrompt("废土风 超市\n【视觉风格】已有风格包");
        expect(withStyle.prompt.match(/【视觉风格】/g)?.length).toBe(1);
    });

    test("video mode keeps style but skips still-image job templates", () => {
        const { prompt } = enhanceCanvasGenerationPrompt("废土风 山姆超市 设定图 纯白背景", "video");
        expect(prompt).toContain("【视觉风格】末日废土");
        expect(prompt).not.toContain("【作业】场景设定图");
        expect(prompt).not.toContain("纯白无缝白底");
    });
});

describe("video prompt polish", () => {
    test("adds a director note for short prompts and leaves long or tagged prompts alone", () => {
        expect(isCanvasPromptOptimizeEnabled(undefined)).toBe(true);
        expect(isCanvasPromptOptimizeEnabled("false")).toBe(false);
        const polished = polishVideoPrompt("女孩在雨里走路");
        expect(polished).toContain("女孩在雨里走路");
        expect(polished).toContain("【镜头设计】");
        expect(polishVideoPrompt("女孩\n【镜头设计】已有")).not.toContain("按可拍摄镜头补全");
        expect(polishVideoPrompt("x".repeat(240))).toBe("x".repeat(240));
    });
});

describe("canvas generation variation and seed", () => {
    test("keeps the first sample closest to the user prompt", () => {
        const base = "废土风 山姆超市 设定图";
        const intent = detectCanvasGenerationIntent(base);
        expect(varyCanvasGenerationPrompt(base, 0, 4, intent)).toBe(base);
        expect(varyCanvasGenerationPrompt(base, 1, 4, intent)).toContain("【探索变体】");
        expect(varyCanvasGenerationPrompt(base, 1, 1, intent)).toBe(base);
    });

    test("unique seeds per batch index share a salt but do not collide", () => {
        const salt = 42;
        const seeds = [0, 1, 2, 3].map((index) => canvasGenerationSeed(index, salt));
        expect(new Set(seeds).size).toBe(4);
        expect(seeds.every((seed) => seed > 0)).toBe(true);
    });
});
