import { describe, expect, test } from "bun:test";

import { CANVAS_IMAGE_BATCH_MAX_COUNT } from "@oc/lib/canvas/canvas-generation-count";
import { applyDeckDisplayConnections, applyImageDeckUploads, collectImageDeckCopyIds, expandMentionResourceNodes, getImageDeckMembers, getImageDeckRoot, IMAGE_UPLOAD_GAP, imageDeckFanMotion, isImageDeckRoot, mentionableDeckNodes, preserveCopiedImageDeck, resolveDeckConnection, resolveFannedDeckRootId, retargetCollapsedDeckNodeId, syncImageDeckAfterRemoval } from "./canvas-image-stack";
import { getContextResourceNodes, getGenerationResourceNodes, getMentionResourceNodes } from "./canvas-resource-references";
import { removeCanvasNodes } from "./canvas-project-domain";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type CanvasNodeMetadata } from "@oc/types/canvas";

function image(id: string, extra?: Partial<CanvasNodeData> & { metadata?: CanvasNodeMetadata }): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Image,
        title: id,
        position: extra?.position || { x: 0, y: 0 },
        width: extra?.width ?? 100,
        height: extra?.height ?? 80,
        parentId: extra?.parentId,
        metadata: extra?.metadata,
    };
}

function upload(id: string, title = id) {
    return {
        id,
        title,
        size: { width: 120, height: 90 },
        metadata: { content: `url:${id}`, storageKey: `key:${id}`, status: "success" as const },
    };
}

describe("canvas image deck stack", () => {
    test("places extra uploads as independent nodes beside an existing image node", () => {
        const target = image("root", { position: { x: 40, y: 20 }, width: 200, height: 160, metadata: { content: "" } });
        const result = applyImageDeckUploads([target], {
            items: [upload("a", "one.png"), upload("b", "two.png"), upload("c", "three.png")],
            origin: { x: 400, y: 300 },
            targetNodeId: "root",
        });

        const root = result.nodes.find((node) => node.id === "root")!;
        expect(isImageDeckRoot(root)).toBe(false);
        expect(root.title).toBe("one.png");
        expect(root.width).toBe(200);
        expect(root.metadata?.imageStackLayout).toBeUndefined();
        expect(root.metadata?.batchChildIds).toBeUndefined();
        expect(result.addedIds).toEqual(["b", "c"]);
        expect(result.nodes.find((node) => node.id === "b")?.position).toEqual({ x: 40 + 200 + IMAGE_UPLOAD_GAP, y: 20 });
        expect(result.nodes.find((node) => node.id === "c")?.metadata?.batchRootId).toBeUndefined();
        expect(result.nodes.find((node) => node.id === "c")?.position.x).toBeGreaterThan(result.nodes.find((node) => node.id === "b")!.position.x);
    });

    test("creates independent nodes on blank canvas and truncates past the batch cap", () => {
        const items = Array.from({ length: 10 }, (_, index) => upload(`img-${index}`));
        const result = applyImageDeckUploads([], { items, origin: { x: 100, y: 80 } });
        expect(result.truncated).toBe(10 - CANVAS_IMAGE_BATCH_MAX_COUNT);
        expect(result.addedIds).toHaveLength(CANVAS_IMAGE_BATCH_MAX_COUNT);
        const root = result.nodes.find((node) => node.id === result.rootId)!;
        expect(isImageDeckRoot(root)).toBe(false);
        expect(result.nodes.every((node) => !node.metadata?.imageStackLayout && !node.metadata?.batchRootId)).toBe(true);
        expect(root.position).toEqual({ x: 40, y: 35 });
        const second = result.nodes.find((node) => node.id === result.addedIds[1])!;
        expect(second.position.x).toBe(root.position.x + root.width + IMAGE_UPLOAD_GAP);
    });

    test("dropping more images onto a node replaces it and places the rest independently", () => {
        const seeded = applyImageDeckUploads([image("root", { position: { x: 0, y: 0 } })], {
            items: [upload("a"), upload("b")],
            origin: { x: 0, y: 0 },
            targetNodeId: "root",
        });
        const result = applyImageDeckUploads(seeded.nodes, {
            items: [upload("x", "child.png"), upload("y"), upload("z")],
            origin: { x: 0, y: 0 },
            targetNodeId: "b",
        });
        const root = result.nodes.find((node) => node.id === "root")!;
        expect(root.metadata?.batchChildIds).toBeUndefined();
        expect(result.nodes.find((node) => node.id === "b")?.title).toBe("child.png");
        expect(result.addedIds).toEqual(["y", "z"]);
        expect(result.nodes.find((node) => node.id === "y")?.metadata?.batchRootId).toBeUndefined();
    });

    test("fan motion spreads later cards further to the right", () => {
        const first = imageDeckFanMotion(0, 4);
        const last = imageDeckFanMotion(3, 4);
        expect(first).toEqual({ x: 0, y: 0, rotate: 0, index: 0 });
        expect(last.x).toBeGreaterThan(first.x);
        expect(last.y).toBeLessThan(0);
        expect(first.rotate).toBe(0);
        expect(last.rotate).toBe(0);
    });

    test("collapsed connections retarget onto the deck root and hide intra-deck edges", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], content: "r" } });
        const child = image("child", { metadata: { batchRootId: "root", imageStackLayout: "deck", content: "c" } });
        const video = image("video", { metadata: { content: "v" } });
        video.type = CanvasNodeType.Video;
        const nodes = [root, child, video];
        const collapsed = resolveDeckConnection({ id: "e1", fromNodeId: "child", toNodeId: "video" }, child, video, nodes, null);
        expect(collapsed?.from.id).toBe("root");
        expect(collapsed?.to.id).toBe("video");
        expect(resolveDeckConnection({ id: "e2", fromNodeId: "root", toNodeId: "child" }, root, child, nodes, null)).toBeNull();
        const fanned = resolveDeckConnection({ id: "e1", fromNodeId: "child", toNodeId: "video" }, child, video, nodes, "root");
        expect(fanned?.from.id).toBe("child");
        expect(retargetCollapsedDeckNodeId("child", nodes, null)).toBe("root");
        expect(retargetCollapsedDeckNodeId("child", nodes, "root")).toBe("child");
    });

    test("display connections remap when stacked and follow fan offsets when open", () => {
        const root = image("root", { position: { x: 10, y: 10 }, metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], content: "r" } });
        const child = image("child", { position: { x: 24, y: 18 }, metadata: { batchRootId: "root", imageStackLayout: "deck", content: "c" } });
        const video = { ...image("video", { position: { x: 400, y: 10 } }), type: CanvasNodeType.Video };
        const items = [{ connection: { id: "e", fromNodeId: "child", toNodeId: "video" }, from: child, to: video }];
        const stacked = applyDeckDisplayConnections(items, [root, child, video], null);
        expect(stacked[0]?.from.id).toBe("root");
        expect(stacked[0]?.from.position).toEqual(root.position);
        const fanned = applyDeckDisplayConnections(items, [root, child, video], "root");
        expect(fanned[0]?.from.id).toBe("child");
        expect(fanned[0]?.from.position.x).toBeGreaterThan(child.position.x);
    });

    test("a connected deck root lists every member for @ mentions", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], content: "r" } });
        const child = image("child", { metadata: { batchRootId: "root", imageStackLayout: "deck", content: "c" } });
        const video = { ...image("video"), type: CanvasNodeType.Video };
        const nodes = [root, child, video];
        expect(mentionableDeckNodes(root, nodes).map((node) => node.id)).toEqual(["root", "child"]);
        expect(mentionableDeckNodes(child, nodes).map((node) => node.id)).toEqual(["child"]);
        const mentioned = expandMentionResourceNodes([root], nodes);
        expect(mentioned.map((node) => node.id)).toEqual(["root", "child"]);
    });

    test("independent connected images are mentionable without deck fields", () => {
        const video = { ...image("video"), type: CanvasNodeType.Video };
        const photo = image("photo", { metadata: { content: "/api/video-canvas/media/abc", storageKey: "resource:abc" } });
        const nodes = [photo, video];
        const connections: CanvasConnection[] = [{ id: "e", fromNodeId: "photo", toNodeId: "video" }];
        expect(mentionableDeckNodes(photo, nodes).map((node) => node.id)).toEqual(["photo"]);
        expect(getMentionResourceNodes("video", nodes, connections).map((node) => node.id)).toEqual(["photo"]);
        expect(getGenerationResourceNodes("video", nodes, connections).map((node) => node.id)).toEqual(["photo"]);
    });

    test("generation resources stay on the connected root unless the caller expands mentions", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], content: "r" } });
        const child = image("child", { metadata: { batchRootId: "root", imageStackLayout: "deck", content: "c" } });
        const video = { ...image("video"), type: CanvasNodeType.Video };
        const nodes = [root, child, video];
        const connections: CanvasConnection[] = [{ id: "e", fromNodeId: "root", toNodeId: "video" }];
        expect(getContextResourceNodes("video", nodes, connections).map((node) => node.id)).toEqual(["root"]);
        expect(getGenerationResourceNodes("video", nodes, connections).map((node) => node.id)).toEqual(["root"]);
        expect(getMentionResourceNodes("video", nodes, connections).map((node) => node.id)).toEqual(["root", "child"]);
    });

    test("hovering a child fans that deck", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"] } });
        const child = image("child", { metadata: { batchRootId: "root", imageStackLayout: "deck" } });
        const nodes = [root, child];
        expect(resolveFannedDeckRootId({ hoveredNodeId: "child", nodes })).toBe("root");
        expect(resolveFannedDeckRootId({ selectedNodeIds: ["child"], nodes })).toBe("root");
        expect(resolveFannedDeckRootId({ hoveredNodeId: "other", nodes })).toBeNull();
        expect(getImageDeckRoot(child, nodes)?.id).toBe("root");
    });

    test("copying a complete deck remaps membership instead of isolating it", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], primaryImageId: "root", content: "r" } });
        const child = image("child", { metadata: { batchRootId: "root", imageStackLayout: "deck", content: "c" } });
        const copied = collectImageDeckCopyIds(["root"], [root, child]);
        expect([...copied].sort()).toEqual(["child", "root"]);
        const idMap = new Map([["root", "root-copy"], ["child", "child-copy"]]);
        expect(preserveCopiedImageDeck(root, idMap, copied)).toEqual({
            isBatchRoot: true,
            imageStackLayout: "deck",
            batchChildIds: ["child-copy"],
            primaryImageId: "root-copy",
            imageBatchExpanded: false,
            batchRootId: undefined,
        });
        expect(preserveCopiedImageDeck(child, idMap, copied)?.batchRootId).toBe("root-copy");
        expect(preserveCopiedImageDeck(root, idMap, new Set(["root"]))).toBeNull();
    });

    test("dropping onto a generation grid batch creates a separate deck", () => {
        const gridRoot = image("grid", { metadata: { isBatchRoot: true, imageStackLayout: "grid", batchChildIds: ["g1"], content: "grid" } });
        const gridChild = image("g1", { metadata: { batchRootId: "grid", content: "g1" } });
        const result = applyImageDeckUploads([gridRoot, gridChild], {
            items: [upload("a"), upload("b")],
            origin: { x: 200, y: 100 },
            targetNodeId: "grid",
        });
        expect(result.nodes.find((node) => node.id === "grid")?.metadata?.imageStackLayout).toBe("grid");
        expect(result.nodes.find((node) => node.id === "grid")?.metadata?.batchChildIds).toEqual(["g1"]);
        expect(isImageDeckRoot(result.nodes.find((node) => node.id === result.rootId)!)).toBe(false);
        expect(result.rootId).not.toBe("grid");
        expect(result.addedIds).toHaveLength(2);
    });

    test("removing the last deck child dissolves the stack metadata", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], primaryImageId: "child", content: "r" } });
        const dissolved = syncImageDeckAfterRemoval(root, []);
        expect(dissolved.metadata?.isBatchRoot).toBeUndefined();
        expect(dissolved.metadata?.imageStackLayout).toBeUndefined();
        expect(dissolved.metadata?.batchChildIds).toBeUndefined();
        expect(syncImageDeckAfterRemoval(root, ["child"]).metadata?.primaryImageId).toBe("child");
        expect(syncImageDeckAfterRemoval(root, ["other"]).metadata?.primaryImageId).toBe("root");
    });

    test("deleting a deck child does not copy its media onto the root", () => {
        const root = image("root", { metadata: { isBatchRoot: true, imageStackLayout: "deck", batchChildIds: ["child"], primaryImageId: "child", content: "root-media" } });
        const child = image("child", { metadata: { batchRootId: "root", imageStackLayout: "deck", content: "child-media" } });
        const result = removeCanvasNodes([root, child], new Set(["child"]));
        const nextRoot = result.nodes.find((node) => node.id === "root")!;
        expect(nextRoot.metadata?.content).toBe("root-media");
        expect(nextRoot.metadata?.imageStackLayout).toBeUndefined();
        expect(nextRoot.metadata?.isBatchRoot).toBeUndefined();
    });
});
