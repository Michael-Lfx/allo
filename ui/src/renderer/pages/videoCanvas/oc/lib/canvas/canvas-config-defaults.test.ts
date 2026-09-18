import { describe, expect, test } from "bun:test";

import { defaultVideoOperation } from "./canvas-config-defaults";

describe("defaultVideoOperation", () => {
    test("falls back to text-to-video when there are no media inputs", () => {
        expect(defaultVideoOperation({ textCount: 1, imageCount: 0, videoCount: 0, audioCount: 0 })).toBe("text_to_video");
    });

    test("prefers image-to-video when images are connected", () => {
        expect(defaultVideoOperation({ textCount: 0, imageCount: 1, videoCount: 0, audioCount: 0 })).toBe("image_to_video");
    });

    test("prefers reference-to-video for three or more images", () => {
        expect(defaultVideoOperation({ textCount: 0, imageCount: 3, videoCount: 0, audioCount: 0 })).toBe("reference_to_video");
    });
});
