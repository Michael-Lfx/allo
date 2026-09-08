import { describe, expect, test } from "bun:test";

import {
    canvasMediaSpecsForModel,
    defaultModelCapabilityConfig,
    imageCapabilityConfigFor,
    imageSizeToAspectRatio,
    modelCapabilityConfigFor,
    modelPromptLengthError,
    normalizeImageValue,
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

    test("Wan 3.0 uses DashScope duration and resolution tokens", () => {
        const profile = modelCapabilityConfigFor(configFor("flowy::wan3.0-video"), "flowy::wan3.0-video");
        expect(profile.video?.duration.min).toBe(2);
        expect(profile.video?.duration.max).toBe(30);
        expect(profile.video?.duration.default).toBe(5);
        expect(profile.video?.resolutions).toEqual(["480P", "720P", "1080P"]);
        expect(profile.video?.defaultResolution).toBe("720P");
        expect(profile.video?.generateAudio.supported).toBe(true);
        expect(profile.video?.operations).toContain("extend");
    });

    test("generic video models default to reference-video capacity", () => {
        const profile = modelCapabilityConfigFor(configFor("default::some-generic-video"), "default::some-generic-video");
        expect(profile.video?.references.maxVideos).toBeGreaterThan(0);
        expect(profile.video?.operations).toContain("extend");
        expect(profile.video?.resolutions).not.toContain("2160p");
    });

    test("Grok Imagine video uses xAI ratios and does not advertise extend", () => {
        const profile = modelCapabilityConfigFor(configFor("default::grok-imagine-video"), "default::grok-imagine-video");
        expect(profile.video?.duration.min).toBe(5);
        expect(profile.video?.duration.max).toBe(12);
        expect(profile.video?.ratios).toEqual(["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3"]);
        expect(profile.video?.resolutions).toEqual(["480p", "720p", "1080p"]);
        expect(profile.video?.operations).toEqual(["text_to_video", "image_to_video"]);
        expect(profile.video?.operations).not.toContain("extend");
    });

    test("Kling duration and ratios follow the official enum", () => {
        const profile = modelCapabilityConfigFor(configFor("flowy::kling-v2.1"), "flowy::kling-v2.1");
        expect(profile.video?.duration.selection).toBe("enum");
        expect(profile.video?.duration.values).toEqual([5, 10]);
        expect(profile.video?.ratios).toEqual(["16:9", "9:16", "1:1"]);
        expect(profile.video?.resolutions).toEqual(["720p", "1080p"]);
    });

    test("Sora and Veo expose their duration enums even without a protocol stamp", () => {
        const sora = modelCapabilityConfigFor(configFor("default::sora-2"), "default::sora-2");
        expect(sora.video?.duration.values).toEqual([4, 8, 12]);
        expect(sora.video?.ratios).toEqual(["16:9", "9:16"]);
        const veo = modelCapabilityConfigFor(configFor("default::veo-3"), "default::veo-3");
        expect(veo.video?.duration.values).toEqual([4, 6, 8]);
        expect(veo.video?.resolutions).toEqual(["720p", "1080p"]);
    });

    test("Seedance duration matches the 5–15s engine window", () => {
        const profile = modelCapabilityConfigFor(configFor("flowy::doubao-seedance-2-0"), "flowy::doubao-seedance-2-0");
        expect(profile.video?.duration.min).toBe(5);
        expect(profile.video?.duration.max).toBe(15);
        expect(profile.video?.resolutions).toEqual(["480p", "720p", "1080p"]);
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

describe("image model capability profiles", () => {
    test("Seedream offers official 2K ratios and hides quality/custom pixels", () => {
        const profile = imageCapabilityConfigFor(configFor("flowy::doubao-seedream-5-0"), "flowy::doubao-seedream-5-0");
        expect(profile.aspects.map((item) => item.value)).toEqual(["1:1", "16:9", "9:16", "4:3", "3:4", "21:9"]);
        expect(profile.qualities).toEqual([]);
        expect(profile.customPixels).toBe(false);
        expect(profile.transparentBackground).toBe(false);
        expect(normalizeImageValue(profile, { size: "1024x1024", quality: "high" })).toEqual({ size: "1:1", quality: "auto" });
    });

    test("gpt-image persists OpenAI pixel sizes", () => {
        const profile = imageCapabilityConfigFor(configFor("default::gpt-image-2"), "default::gpt-image-2");
        expect(profile.aspects.some((item) => item.size === "1024x1024")).toBe(true);
        expect(profile.transparentBackground).toBe(true);
        expect(normalizeImageValue(profile, { size: "16:9", quality: "medium" }).size).toBe("1536x1024");
    });

    test("Grok Imagine image uses xAI ratios without 4K", () => {
        const profile = imageCapabilityConfigFor(configFor("default::grok-imagine"), "default::grok-imagine");
        expect(profile.aspects.map((item) => item.value)).toContain("2:1");
        expect(profile.aspects.map((item) => item.value)).not.toContain("21:9");
        expect(profile.qualities).toEqual([]);
    });

    test("imageSizeToAspectRatio maps pixels and 4k aliases for Flowy submit", () => {
        expect(imageSizeToAspectRatio("1024x1024")).toBe("1:1");
        expect(imageSizeToAspectRatio("16:9-4k")).toBe("16:9");
        expect(imageSizeToAspectRatio("auto")).toBe("");
        expect(imageSizeToAspectRatio("2816x1584")).toBe("16:9");
    });

    test("switching a leftover image size onto Kling remaps duration and ratio", () => {
        const specs = canvasMediaSpecsForModel(configFor("flowy::kling-v2.1"), "flowy::kling-v2.1", "video", {
            size: "21:9",
            seconds: "3",
            vquality: "2160",
        });
        expect(specs.size).toBe("16:9");
        expect(specs.seconds).toBe("5");
        expect(specs.vquality).toBe("1080");
    });
});
