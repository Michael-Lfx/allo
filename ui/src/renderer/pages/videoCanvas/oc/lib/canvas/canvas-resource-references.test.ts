import { describe, expect, test } from "bun:test";

import { canvasMediaUrl } from "@renderer/pages/videoCanvas/api";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";

import { buildNodeMentionReferences, canvasResourceNodePreviewUrl } from "./canvas-resource-references";

const imageNode = (id: string, metadata: Record<string, unknown>): CanvasNodeData =>
    ({
        id,
        type: CanvasNodeType.Image,
        title: id,
        position: { x: 0, y: 0 },
        width: 100,
        height: 100,
        metadata,
    }) as CanvasNodeData;

describe("canvasResourceNodePreviewUrl", () => {
    test("rewrites a stale-port content URL onto the current media endpoint", () => {
        const node = imageNode("img", { content: "http://127.0.0.1:11111/api/video-canvas/media/mid-9" });
        expect(canvasResourceNodePreviewUrl(node)).toBe(canvasMediaUrl("mid-9"));
    });

    test("paints from storageKey when content is a dead blob", () => {
        const node = imageNode("img", {
            storageKey: "resource:stored",
            content: "blob:http://127.0.0.1:5173/dead",
        });
        expect(canvasResourceNodePreviewUrl(node)).toBe(canvasMediaUrl("stored"));
    });
});

describe("buildNodeMentionReferences previewUrl", () => {
    test("connected image thumbs use the live media URL, not a dead blob", () => {
        const target = imageNode("video-target", { content: "" });
        target.type = CanvasNodeType.Video;
        const source = imageNode("image-a", {
            storageKey: "resource:mid-9",
            content: "blob:http://127.0.0.1:5173/dead",
        });
        const connection: CanvasConnection = { id: "c1", fromNodeId: source.id, toNodeId: target.id };
        const [reference] = buildNodeMentionReferences(target, [source, target], [connection]);
        expect(reference?.previewUrl).toBe(canvasMediaUrl("mid-9"));
    });
});
