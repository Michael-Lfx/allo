import { describe, expect, test } from "bun:test";

import { canvasMediaUrl } from "@renderer/pages/videoCanvas/api";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import { canvasAssetDisplayUrl, canvasNodeDisplayUrl, canvasNodeMediaId, rewriteCanvasDisplayUrl } from "./canvas-media-id";

const node = (type: CanvasNodeType, metadata: Record<string, unknown>): CanvasNodeData =>
    ({
        id: "n1",
        type,
        title: "t",
        position: { x: 0, y: 0 },
        width: 320,
        height: 180,
        metadata,
    }) as CanvasNodeData;

describe("rewriteCanvasDisplayUrl", () => {
    test("drops one-shot blob: URLs", () => {
        expect(rewriteCanvasDisplayUrl("blob:http://127.0.0.1:5173/abc")).toBe("");
    });

    test("rewrites resource: keys to the current media endpoint", () => {
        expect(rewriteCanvasDisplayUrl("resource:mid-1")).toBe(canvasMediaUrl("mid-1"));
    });

    test("rewrites stale-port desktop media URLs to the current origin", () => {
        expect(rewriteCanvasDisplayUrl("http://127.0.0.1:59999/api/video-canvas/media/mid-2")).toBe(
            canvasMediaUrl("mid-2")
        );
    });

    test("keeps data: and external https URLs", () => {
        expect(rewriteCanvasDisplayUrl("data:image/png;base64,abc")).toBe("data:image/png;base64,abc");
        expect(rewriteCanvasDisplayUrl("https://cdn.example.com/still.jpg")).toBe(
            "https://cdn.example.com/still.jpg"
        );
    });
});

describe("canvasNodeMediaId / canvasNodeDisplayUrl", () => {
    test("reads resource: content when storageKey is missing", () => {
        const image = node(CanvasNodeType.Image, { content: "resource:from-content" });
        expect(canvasNodeMediaId(image)).toBe("from-content");
        expect(canvasNodeDisplayUrl(image)).toBe(canvasMediaUrl("from-content"));
    });

    test("prefers mediaId over a stale absolute content URL", () => {
        const image = node(CanvasNodeType.Image, {
            mediaId: "live-id",
            content: "http://127.0.0.1:11111/api/video-canvas/media/stale-id",
        });
        expect(canvasNodeMediaId(image)).toBe("live-id");
        expect(canvasNodeDisplayUrl(image)).toBe(canvasMediaUrl("live-id"));
    });

    test("paints from storageKey when content is a dead blob", () => {
        const image = node(CanvasNodeType.Image, {
            storageKey: "resource:stored",
            content: "blob:http://127.0.0.1:5173/dead",
        });
        expect(canvasNodeDisplayUrl(image)).toBe(canvasMediaUrl("stored"));
    });

    test("returns empty when only a blob remains", () => {
        expect(
            canvasNodeDisplayUrl(node(CanvasNodeType.Image, { content: "blob:http://localhost/x" }))
        ).toBe("");
    });
});

describe("canvasAssetDisplayUrl", () => {
    test("prefers storageKey over a stale coverUrl blob", () => {
        expect(
            canvasAssetDisplayUrl({
                kind: "image",
                coverUrl: "blob:http://127.0.0.1:5173/dead-cover",
                data: { storageKey: "resource:live-id", dataUrl: "blob:http://127.0.0.1:5173/dead-data" },
            })
        ).toBe(canvasMediaUrl("live-id"));
    });

    test("rewrites a relative media path onto the current origin", () => {
        expect(
            canvasAssetDisplayUrl({
                kind: "video",
                coverUrl: "/api/video-canvas/media/stale-cover",
                data: { url: "/api/video-canvas/media/vid-1" },
            })
        ).toBe(canvasMediaUrl("vid-1"));
    });

    test("keeps a live session object URL when there is no media id", () => {
        expect(
            canvasAssetDisplayUrl({
                kind: "image",
                coverUrl: "blob:http://127.0.0.1:5173/dead-cover",
                data: { dataUrl: "blob:http://127.0.0.1:5173/session" },
            })
        ).toBe("blob:http://127.0.0.1:5173/session");
    });
});
