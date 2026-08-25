import { describe, expect, test } from "bun:test";

import { alloBodyFromCreateInput, collectMediaIds } from "./task-center";

describe("collectMediaIds / alloBodyFromCreateInput", () => {
    const refA = { id: "a", name: "a.png", type: "image/png", dataUrl: "", storageKey: "resource:media-a" };
    const refB = { id: "b", name: "b.png", type: "image/png", dataUrl: "", storageKey: "resource:media-b" };

    test("image mode keeps every reference image in reference_media_ids (img2img)", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_image",
            operation: "image",
            prompt: "edit the connected image",
            input: {
                mode: "image",
                prompt: "edit the connected image",
                referenceImages: [refA],
            },
        });
        expect(body.mode).toBe("image");
        expect(body.reference_media_ids).toEqual(["media-a"]);
        expect(body.first_frame_media_id).toBeUndefined();
    });

    test("image mode keeps multiple reference images without promoting the first to a frame", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_image",
            prompt: "multi-ref edit",
            input: {
                mode: "image",
                prompt: "multi-ref edit",
                referenceImages: [refA, refB],
            },
        });
        expect(body.reference_media_ids).toEqual(["media-a", "media-b"]);
        expect(body.first_frame_media_id).toBeUndefined();
    });

    test("video mode still promotes the first reference image to first_frame when unset", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_video",
            operation: "image_to_video",
            prompt: "animate",
            input: {
                mode: "video",
                prompt: "animate",
                referenceImages: [refA, refB],
            },
        });
        expect(body.mode).toBe("video");
        expect(body.first_frame_media_id).toBe("media-a");
        expect(body.reference_media_ids).toEqual(["media-b"]);
    });

    test("explicit first-frame metadata is honored and excluded from references", () => {
        const collected = collectMediaIds(
            {
                referenceImages: [refA, refB],
                metadata: { firstFrameMediaId: "media-b" },
            },
            { promoteFirstImageToFrame: true },
        );
        expect(collected.firstFrameId).toBe("media-b");
        expect(collected.referenceIds).toEqual(["media-a"]);
    });
});
