import { describe, expect, test } from "bun:test";

import { defaultConfig, type AiConfig } from "@oc/stores/use-config-store";
import { canvasModelSupportsVision, canvasVisionModelOptions, resolveCanvasVisionModel } from "@oc/lib/canvas/canvas-vision-model";

function visionConfig(overrides: Partial<AiConfig> = {}): AiConfig {
    return {
        ...defaultConfig,
        textModel: "allo-chat::qwen3-vl",
        textModels: ["allo-chat::qwen3-vl", "allo-chat::gpt-5.5"],
        channels: [
            {
                id: "allo-chat",
                name: "Flowy Cloud",
                baseUrl: "/api/video-canvas/llm/v1",
                apiKey: "system",
                apiFormat: "openai",
                models: ["qwen3-vl", "gpt-5.5"],
                modelCosts: [
                    {
                        model: "qwen3-vl",
                        capability: "text",
                        billingMode: "fixed_request",
                        unitPriceMicrocredits: 0,
                        supportsVision: true,
                    },
                    {
                        model: "gpt-5.5",
                        capability: "text",
                        billingMode: "fixed_request",
                        unitPriceMicrocredits: 0,
                    },
                ],
            },
        ],
        ...overrides,
    };
}

describe("resolveCanvasVisionModel", () => {
    test("prefers the selected text model when it accepts image input", () => {
        const config = visionConfig();
        expect(canvasModelSupportsVision(config, "allo-chat::qwen3-vl")).toBe(true);
        expect(resolveCanvasVisionModel(config)).toBe("allo-chat::qwen3-vl");
        expect(canvasVisionModelOptions(config)).toEqual(["allo-chat::qwen3-vl"]);
    });

    test("falls back to the first multimodal chat model", () => {
        const config = visionConfig({ textModel: "allo-chat::gpt-5.5" });
        expect(canvasModelSupportsVision(config, config.textModel)).toBe(false);
        expect(resolveCanvasVisionModel(config)).toBe("allo-chat::qwen3-vl");
    });

    test("returns empty when the catalog has no image-capable chat models", () => {
        const config = visionConfig({
            textModel: "allo-chat::gpt-5.5",
            textModels: ["allo-chat::gpt-5.5"],
            channels: [
                {
                    id: "allo-chat",
                    name: "Flowy Cloud",
                    baseUrl: "/api/video-canvas/llm/v1",
                    apiKey: "system",
                    apiFormat: "openai",
                    models: ["gpt-5.5"],
                    modelCosts: [
                        {
                            model: "gpt-5.5",
                            capability: "text",
                            billingMode: "fixed_request",
                            unitPriceMicrocredits: 0,
                        },
                    ],
                },
            ],
        });
        expect(resolveCanvasVisionModel(config)).toBe("");
    });
});
