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
    url: "blob:uploaded-preview",
    storageKey: "image:u1:abc",
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
}));

bunMock.mock.module("@oc/services/file-storage", () => ({
    resolveMediaUrl: async (storageKey: string, fallback = "") => {
        calls.push(`resolveMediaUrl:${storageKey}`);
        return `object-url:${storageKey}`;
    },
    getMediaBlob: async () => null,
    uploadMediaFile: async (input: string | Blob) => ({
        url: "http://local/uploaded",
        storageKey: "file:u1:x",
        bytes: 0,
        mimeType: "application/octet-stream",
    }),
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

    test("uploads legacy data URLs and stamps upload metadata", async () => {
        calls.length = 0;
        const [hydrated] = await hydrateCanvasImages([
            node(CanvasNodeType.Image, { content: "data:image/png;base64,AAAA" }),
        ]);
        expect(calls).toEqual(["uploadImage:data:image/png;base64,AA"]);
        expect(hydrated.metadata?.content).toBe("blob:uploaded-preview");
        expect(hydrated.metadata?.storageKey).toBe("image:u1:abc");
        expect(hydrated.metadata?.bytes).toBe(1024);
    });
});
