import { describe, expect, test } from "bun:test";

import { listDirectorEnvironmentSources } from "./director-environment";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

describe("listDirectorEnvironmentSources", () => {
    test("收集图片节点与全景节点（含上游图）", () => {
        const nodes: CanvasNodeData[] = [
            { id: "img-1", type: CanvasNodeType.Image, title: "广场", position: { x: 0, y: 0 }, width: 200, height: 120, metadata: { content: "https://example.com/plaza.jpg" } },
            { id: "pano-1", type: CanvasNodeType.Panorama, title: "片场", position: { x: 0, y: 0 }, width: 200, height: 120, metadata: {} },
            { id: "pano-empty", type: CanvasNodeType.Panorama, title: "空全景", position: { x: 0, y: 0 }, width: 200, height: 120, metadata: {} },
        ];
        const sources = listDirectorEnvironmentSources(nodes, [{ id: "c1", fromNodeId: "img-1", toNodeId: "pano-1" }]);
        expect(sources).toEqual([
            { nodeId: "img-1", title: "广场", url: "https://example.com/plaza.jpg" },
            { nodeId: "pano-1", title: "片场", url: "https://example.com/plaza.jpg" },
        ]);
    });
});
