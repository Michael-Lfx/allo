/**
 * Hydrate media policy for opened canvas projects:
 * - remote `resource:` media pass through untouched (no HEAD, no blob: writes,
 *   no cache-miss downloads) — display goes through useNodeResourceUrl;
 * - local `image:`/`video:`/`audio:` storage keys still resolve to object URLs;
 * - legacy `data:image/` payloads are uploaded and stamped back.
 */
import { describe, expect, test } from "bun:test";

type BunMockModule = { module: (specifier: string, factory: () => unknown) => void };
const bunMock = (await import("bun:test")) as unknown as typeof import("bun:test") & {
    mock: BunMockModule;
};

const calls: string[] = [];
const uploaded = {
    url: "/api/video-canvas/media/uploaded-1",
    storageKey: "resource:uploaded-1",
    width: 640,
    height: 360,
    bytes: 1024,
    mimeType: "image/png",
};

bunMock.mock.module("@oc/services/image-storage", () => ({
    resolveImageUrl: async (storageKey: string, fallback = "") => {
        calls.push(`resolveImageUrl:${storageKey}`);
        return `object-url:${storageKey}`;
    },
    uploadImage: async (input: string | Blob) => {
        calls.push(`uploadImage:${typeof input === "string" ? input.slice(0, 24) : "blob"}`);
        return uploaded;
    },
    getImageBlob: async () => null,
    fetchImageSourceBlob: async () => new Blob(["x"], { type: "image/png" }),
}));

bunMock.mock.module("@oc/services/file-storage", () => ({
    resolveMediaUrl: async (storageKey: string, fallback = "") => {
        calls.push(`resolveMediaUrl:${storageKey}`);
        return `object-url:${storageKey}`;
    },
    getMediaBlob: async () => null,
    uploadMediaFile: async (input: string | Blob) => {
        calls.push(`uploadMediaFile:${typeof input === "string" ? input.slice(0, 40) : "blob"}`);
        return {
            url: "/api/video-canvas/media/uploaded-media",
            storageKey: "resource:uploaded-media",
            bytes: 0,
            mimeType: "video/mp4",
        };
    },
}));

// mock.module must be registered before the module under test loads.
const { hydrateCanvasImages } = await import("./canvas-project-generation");
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

const node = (type: CanvasNodeType, metadata: Record<string, unknown>): CanvasNodeData =>
    ({ id: `n-${Math.random()}`, type, title: "t", position: { x: 0, y: 0 }, metadata }) as CanvasNodeData;

describe("hydrateCanvasImages", () => {
    test("keeps remote resource media untouched (no resolve, no upload)", async () => {
        calls.length = 0;
        const nodes = [
            node(CanvasNodeType.Image, {
                content: "/api/video-canvas/media/m1",
                storageKey: "resource:m1",
            }),
            node(CanvasNodeType.Video, {
                content: "/api/video-canvas/media/m2",
                storageKey: "resource:m2",
            }),
            node(CanvasNodeType.Audio, {
                content: "/api/video-canvas/media/m3",
                storageKey: "resource:m3",
            }),
        ];
        const hydrated = await hydrateCanvasImages(nodes);
        expect(hydrated.map((n) => n.metadata?.content)).toEqual([
            "/api/video-canvas/media/m1",
            "/api/video-canvas/media/m2",
            "/api/video-canvas/media/m3",
        ]);
        expect(calls).toEqual([]);
    });

    test("resolves local image storage keys without cache-miss downloads", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Image, {
                content: "fallback",
                storageKey: "image:u1:local",
            }),
        ]);
        expect(calls).toEqual(["resolveImageUrl:image:u1:local"]);
        expect(hydrated.metadata?.content).toBe("object-url:image:u1:local");
    });

    test("resolves local video storage keys through resolveMediaUrl", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Video, {
                content: "fallback",
                storageKey: "video:u1:clip",
            }),
        ]);
        expect(calls).toEqual(["resolveMediaUrl:video:u1:clip"]);
        expect(hydrated.metadata?.content).toBe("object-url:video:u1:clip");
    });

    test("uploads legacy data URLs and stamps canvas media", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Image, { content: "data:image/png;base64,AAAA" }),
        ]);
        expect(calls).toEqual(["uploadImage:data:image/png;base64,AA"]);
        expect(hydrated.metadata?.content).toBe("/api/video-canvas/media/uploaded-1");
        expect(hydrated.metadata?.storageKey).toBe("resource:uploaded-1");
        expect(hydrated.metadata?.mediaId).toBe("uploaded-1");
        expect(hydrated.metadata?.bytes).toBe(1024);
    });

    test("resolves empty content plus a local IndexedDB key for display", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Image, {
                content: "",
                storageKey: "image:u1:empty",
            }),
        ]);
        expect(calls).toEqual(["resolveImageUrl:image:u1:empty"]);
        expect(hydrated.metadata?.content).toBe("object-url:image:u1:empty");
    });

    test("ingests loopback HTTP stills into canvas media", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Image, {
                content: "http://127.0.0.1:18080/female_face_lock_v2.png",
            }),
        ]);
        expect(calls).toEqual(["uploadImage:http://127.0.0.1:18080/f"]);
        expect(hydrated.metadata?.storageKey).toBe("resource:uploaded-1");
        expect(hydrated.metadata?.mediaId).toBe("uploaded-1");
    });

    test("ingests loopback HTTP clips through uploadMediaFile", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Video, {
                content: "http://127.0.0.1:18080/clip.mp4",
            }),
        ]);
        expect(calls).toEqual(["uploadMediaFile:http://127.0.0.1:18080/clip.mp4"]);
        expect(hydrated.metadata?.storageKey).toBe("resource:uploaded-media");
    });
});
