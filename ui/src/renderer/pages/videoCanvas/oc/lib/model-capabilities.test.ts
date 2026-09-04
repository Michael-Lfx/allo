import { describe, expect, test } from "bun:test";

import {
    defaultModelCapabilityConfig,
    modelCapabilityConfigFor,
    modelPromptLengthError,
    resolveModelVideoBooleanOptions,
    VIDEO_REFERENCE_OPERATIONS,
} from "./model-capabilities";

function configFor(model: string, extra?: { protocol?: "volcengine-ark-video" | "xai-video"; maxVideos?: number }) {
    const separator = model.indexOf("::");
    const channelId = separator >= 0 ? model.slice(0, separator) : "default";
    const modelName = separator >= 0 ? model.slice(separator + 2) : model;
    return {
        channels: [
            {
                id: channelId,
                models: [modelName],
                modelCosts: extra
                    ? [
                          {
                              model: modelName,
                              protocol: extra.protocol,
                              capabilityConfig:
                                  extra.maxVideos === undefined
                                      ? undefined
                                      : {
                                            version: 1,
                                            video: {
                                                ...defaultModelCapabilityConfig().video!,
                                                references: {
                                                    ...defaultModelCapabilityConfig().video!.references,
                                                    maxVideos: extra.maxVideos,
                                                    maxAudios: 0,
                                                },
                                                operations: ["text_to_video", "image_to_video"],
                                            },
                                        },
                          },
                      ]
                    : undefined,
            },
        ],
    };
}

describe("video model reference capabilities", () => {
    test("Seedance supports reference video even without an ark protocol stamp", () => {
        const profile = modelCapabilityConfigFor(configFor("flowy::doubao-seedance-2-0"), "flowy::doubao-seedance-2-0");
        expect(profile.video?.references.maxVideos).toBe(3);
        expect(profile.video?.references.maxAudios).toBe(3);
        for (const operation of VIDEO_REFERENCE_OPERATIONS) {
            expect(profile.video?.operations.includes(operation)).toBe(true);
        }
    });

    test("MiniMax-H3 supports reference video and extend/audio ops", () => {
        const profile = modelCapabilityConfigFor(configFor("flowy::MiniMax-H3"), "flowy::MiniMax-H3");
        expect(profile.video?.references.maxVideos).toBe(3);
        expect(profile.video?.operations).toContain("extend");
        expect(profile.video?.operations).toContain("audio_to_video");
    });

    test("generic video models default to reference-video capacity", () => {
        const profile = modelCapabilityConfigFor(configFor("default::grok-imagine-video"), "default::grok-imagine-video");
        expect(profile.video?.references.maxVideos).toBeGreaterThan(0);
        expect(profile.video?.operations).toContain("extend");
    });

    test("legacy stored maxVideos: 0 does not disable reference video", () => {
        const profile = modelCapabilityConfigFor(
            configFor("custom::some-video", { maxVideos: 0 }),
            "custom::some-video",
        );
        expect(profile.video?.references.maxVideos).toBeGreaterThan(0);
        expect(profile.video?.operations).toContain("extend");
    });
});

describe("video prompt length and capability-gated booleans", () => {
    test("rejects video prompts over the Seedance character limit", () => {
        const model = "flowy::doubao-seedance-2-0";
        const error = modelPromptLengthError(configFor(model), model, "video", "画".repeat(1001));
        expect(error).toContain("1000");
        expect(error).toContain("1001");
        expect(modelPromptLengthError(configFor(model), model, "video", "画".repeat(1000))).toBe("");
    });

    test("forces audio and watermark off when the model does not support them", () => {
        const model = "flowy::MiniMax-H3";
        expect(resolveModelVideoBooleanOptions(configFor(model), model, { videoGenerateAudio: "true", videoWatermark: "true" })).toEqual({
            videoGenerateAudio: "false",
            videoWatermark: "false",
        });
    });

    test("keeps Seedance audio on by default and watermark off unless requested", () => {
        const model = "flowy::doubao-seedance-2-0";
        expect(resolveModelVideoBooleanOptions(configFor(model), model)).toEqual({
            videoGenerateAudio: "true",
            videoWatermark: "false",
        });
        expect(resolveModelVideoBooleanOptions(configFor(model), model, { videoWatermark: "true" }).videoWatermark).toBe("true");
    });
});
