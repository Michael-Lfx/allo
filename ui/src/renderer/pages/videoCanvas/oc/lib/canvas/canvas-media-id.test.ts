import { describe, expect, test } from "bun:test";

import { canvasMediaUrl } from "@renderer/pages/videoCanvas/api";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import { canvasAssetDisplayUrl, canvasNodeDisplayUrl, canvasNodeMediaId, canvasNodeReferenceSource, persistableCanvasNode, referenceImagePreviewUrl, rewriteCanvasDisplayUrl } from "./canvas-media-id";

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

    test("keeps a live blob when a local IndexedDB key can restore it", () => {
        expect(
            canvasNodeDisplayUrl(node(CanvasNodeType.Image, {
                storageKey: "image:u1:local",
                content: "blob:http://127.0.0.1:5173/session",
            }))
        ).toBe("blob:http://127.0.0.1:5173/session");
    });
});

describe("canvasNodeReferenceSource", () => {
    test("keeps inline data URLs on dataUrl", () => {
        const source = canvasNodeReferenceSource(node(CanvasNodeType.Image, { content: "data:image/png;base64,abc" }));
        expect(source?.dataUrl).toBe("data:image/png;base64,abc");
        expect(source?.url).toBeUndefined();
        expect(source?.storageKey).toBeUndefined();
    });

    test("rewrites canvas media onto storageKey and current-origin url", () => {
        const source = canvasNodeReferenceSource(node(CanvasNodeType.Image, {
            content: "http://127.0.0.1:11111/api/video-canvas/media/mid-9",
        }));
        expect(source).toEqual({
            dataUrl: "",
            url: canvasMediaUrl("mid-9"),
            storageKey: "resource:mid-9",
        });
    });

    test("prefers storageKey when content is a dead blob", () => {
        const source = canvasNodeReferenceSource(node(CanvasNodeType.Image, {
            storageKey: "resource:stored",
            content: "blob:http://127.0.0.1:5173/dead",
        }));
        expect(source?.dataUrl).toBe("");
        expect(source?.url).toBe(canvasMediaUrl("stored"));
        expect(source?.storageKey).toBe("resource:stored");
    });

    test("returns null when only a dead blob remains", () => {
        expect(canvasNodeReferenceSource(node(CanvasNodeType.Image, { content: "blob:http://localhost/x" }))).toBeNull();
    });
});

describe("referenceImagePreviewUrl", () => {
    test("prefers storageKey over an empty dataUrl", () => {
        expect(referenceImagePreviewUrl({
            dataUrl: "",
            url: canvasMediaUrl("mid-9"),
            storageKey: "resource:mid-9",
        })).toBe(canvasMediaUrl("mid-9"));
    });

    test("rewrites a stale-port url when dataUrl was cleared for fetch", () => {
        expect(referenceImagePreviewUrl({
            dataUrl: "",
            url: "http://127.0.0.1:11111/api/video-canvas/media/mid-9",
        })).toBe(canvasMediaUrl("mid-9"));
    });

    test("keeps inline data URLs", () => {
        expect(referenceImagePreviewUrl({ dataUrl: "data:image/png;base64,abc" })).toBe("data:image/png;base64,abc");
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

describe("persistableCanvasNode", () => {
    test("rewrites canvas media onto a portable relative path", () => {
        const persisted = persistableCanvasNode(node(CanvasNodeType.Image, {
            mediaId: "mid-7",
            storageKey: "resource:mid-7",
            content: "blob:http://127.0.0.1:5173/dead",
        }));
        expect(persisted.metadata).toEqual({
            mediaId: "mid-7",
            storageKey: "resource:mid-7",
            content: "/api/video-canvas/media/mid-7",
        });
    });

    test("drops an orphan blob and a loopback storageKey", () => {
        const persisted = persistableCanvasNode(node(CanvasNodeType.Image, {
            storageKey: "http://127.0.0.1:18080/face.png",
            content: "blob:http://localhost/x",
        }));
        expect(persisted.metadata?.content).toBe("");
        expect(persisted.metadata?.storageKey).toBeUndefined();
    });
});
