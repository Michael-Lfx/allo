import { describe, expect, test } from "bun:test";

import { canvasMediaUrl } from "@renderer/pages/videoCanvas/api";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import { canvasNodeVideoPreviewUrl } from "./canvas-media-preview";

const video = (metadata: Record<string, unknown>): CanvasNodeData =>
    ({
        id: "v1",
        type: CanvasNodeType.Video,
        title: "clip",
        position: { x: 0, y: 0 },
        width: 320,
        height: 180,
        metadata,
    }) as CanvasNodeData;

describe("canvasNodeVideoPreviewUrl", () => {
    test("does not treat a dead blob poster as a usable image src", () => {
        expect(
            canvasNodeVideoPreviewUrl(
                video({
                    content: "http://127.0.0.1:11111/api/video-canvas/media/vid",
                    videoPreview: { content: "blob:http://127.0.0.1:5173/poster" },
                })
            )
        ).toBe("");
    });

    test("does not return the video file itself as an img src", () => {
        const url = "http://127.0.0.1:11111/api/video-canvas/media/vid";
        expect(canvasNodeVideoPreviewUrl(video({ content: url, previewContent: url }))).toBe("");
    });

    test("rewrites a persisted media poster onto the current origin", () => {
        expect(
            canvasNodeVideoPreviewUrl(
                video({
                    content: "http://127.0.0.1:11111/api/video-canvas/media/vid",
                    videoPreview: { content: "http://127.0.0.1:11111/api/video-canvas/media/poster" },
                })
            )
        ).toBe(canvasMediaUrl("poster"));
    });
});
