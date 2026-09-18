import { describe, expect, test } from "bun:test";

import { applyFrameDrop, buildCanvasFrameDropIndex, canFolderContain, findFrameDropTarget, findFrameDropTargetFromIndex, isCanvasFolderNode } from "./canvas-frame";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

function folder(id: string): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Frame,
        title: "我的文件",
        position: { x: 100, y: 100 },
        width: 360,
        height: 280,
        metadata: {
            frame: { collapsed: true, expandedWidth: 760, expandedHeight: 520 },
            folder: {
                style: "glass",
                createdAt: "2026-08-20T00:00:00.000Z",
            },
        },
    };
}

function image(id: string, content = "resource:image"): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Image,
        title: id,
        position: { x: 150, y: 150 },
        width: 180,
        height: 120,
        metadata: { content },
    };
}

describe("canvas folders", () => {
    test("识别文件夹并允许非容器节点进入本地文件夹", () => {
        const target = folder("folder");
        expect(isCanvasFolderNode(target)).toBe(true);
        expect(canFolderContain(image("image"))).toBe(true);
        expect(canFolderContain(folder("nested"))).toBe(false);
    });

    test("折叠文件夹仍可成为拖放目标并保存展开布局", () => {
        const target = folder("folder");
        const dragged = image("image");
        const nodes = [target, dragged];

        expect(findFrameDropTarget(nodes, new Set([dragged.id]))).toBe(target.id);
        const next = applyFrameDrop(nodes, new Set([dragged.id]), target.id);
        const nextFolder = next.find((node) => node.id === target.id)!;
        const nextImage = next.find((node) => node.id === dragged.id)!;

        expect(nextImage.parentId).toBe(target.id);
        expect(nextFolder.metadata?.frame?.collapsed).toBe(true);
        expect(nextFolder.metadata?.frame?.expandedWidth).toBeGreaterThan(dragged.width);
        expect(nextFolder.metadata?.frame?.expandedHeight).toBeGreaterThan(dragged.height);
    });

    test("空间索引画框落点与常规落点一致", () => {
        const target = folder("folder");
        const dragged = image("image");
        const nodes = [target, dragged];
        const index = buildCanvasFrameDropIndex(nodes);

        expect(findFrameDropTargetFromIndex(index, [dragged], new Set([dragged.id]), { x: 0, y: 0 })).toBe(findFrameDropTarget(nodes, new Set([dragged.id])));
        expect(findFrameDropTargetFromIndex(index, [dragged], new Set([dragged.id]), { x: 400, y: 400 })).toBeNull();
    });

    test("拖出本地文件夹会解除父子关系且不复制未改动节点", () => {
        const child = { ...image("image"), parentId: "folder" };
        const target = folder("folder");
        const nodes = [target, child];
        const next = applyFrameDrop(nodes, new Set([child.id]), null);
        expect(next.find((node) => node.id === child.id)?.parentId).toBeUndefined();
        expect(next.find((node) => node.id === target.id)).toBe(target);
    });
});
