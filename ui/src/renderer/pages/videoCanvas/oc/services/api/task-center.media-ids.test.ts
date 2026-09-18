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

    test("video start/end frame node ids map onto first/last frame media ids", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_video",
            operation: "image_to_video",
            prompt: "animate",
            input: {
                mode: "video",
                prompt: "animate",
                referenceImages: [refA, refB],
                metadata: {
                    videoStartFrameNodeId: "b",
                    videoEndFrameNodeId: "a",
                },
            },
        });
        expect(body.first_frame_media_id).toBe("media-b");
        expect(body.last_frame_media_id).toBe("media-a");
        expect(body.reference_media_ids).toEqual([]);
    });

    test("three video keyframes stay in reference_media_ids instead of first/last frames", () => {
        const refC = { id: "c", name: "c.png", type: "image/png", dataUrl: "", storageKey: "resource:media-c" };
        const body = alloBodyFromCreateInput({
            type: "canvas_video",
            operation: "image_to_video",
            prompt: "animate",
            input: {
                mode: "video",
                prompt: "animate",
                referenceImages: [refA, refB, refC],
                metadata: {
                    videoEditOperation: "image_to_video",
                    videoStartFrameNodeId: "a",
                    videoEndFrameNodeId: "c",
                },
            },
        });
        expect(body.first_frame_media_id).toBeUndefined();
        expect(body.last_frame_media_id).toBeUndefined();
        expect(body.reference_media_ids).toEqual(["media-a", "media-b", "media-c"]);
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

    test("canvas node jobs stamp project_id onto the generation body", () => {
        const body = alloBodyFromCreateInput({
            projectId: "canvas-proj-1",
            type: "canvas_video",
            operation: "text_to_video",
            prompt: "a cat walks",
            input: { mode: "video", prompt: "a cat walks" },
        });
        expect(body.project_id).toBe("canvas-proj-1");
    });

    test("home clip jobs omit project_id", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_video",
            prompt: "a cat walks",
            input: { mode: "video", prompt: "a cat walks" },
        });
        expect(body.project_id).toBeUndefined();
    });

    test("Wan 3.0 clamps duration and keeps DashScope resolution casing", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_video",
            operation: "text_to_video",
            model: "flowy/wan3.0-video",
            prompt: "a cat walks",
            input: {
                mode: "video",
                prompt: "a cat walks",
                config: { videoSeconds: "40", vquality: "720p", size: "16:9" },
            },
        });
        expect(body.duration_secs).toBe(30);
        expect(body.resolution).toBe("720P");
    });

    test("Kling clamps unsupported duration and ultrawide ratio", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_video",
            operation: "text_to_video",
            model: "flowy/kling-v2.1",
            prompt: "a cat walks",
            input: {
                mode: "video",
                prompt: "a cat walks",
                config: { videoSeconds: "3", vquality: "2160p", size: "21:9" },
            },
        });
        expect(body.duration_secs).toBe(5);
        expect(body.aspect_ratio).toBe("16:9");
        expect(body.resolution).toBe("1080p");
    });

    test("image pixel sizes map onto Flowy aspect_ratio", () => {
        const body = alloBodyFromCreateInput({
            type: "canvas_image",
            operation: "image",
            model: "flowy/doubao-seedream-5-0",
            prompt: "a still",
            input: {
                mode: "image",
                prompt: "a still",
                config: { size: "1024x1024" },
            },
        });
        expect(body.mode).toBe("image");
        expect(body.aspect_ratio).toBe("1:1");
    });
});
