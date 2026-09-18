import { describe, expect, test } from "bun:test";

import { createCharacterSubjectNode, findCharacterSubjectNode, isCanvasCharacterSubject } from "./canvas-character-subject";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

const payload = {
    kind: "character" as const,
    title: "林夏",
    assetId: "asset-1",
    versionId: "v1",
    prompt: "角色卡",
    aliases: ["夏夏"],
    definition: { role: "女主" },
    coverUrl: "https://example.com/linxia.png",
    visualStatus: "ready",
    voiceStatus: "ready",
};

describe("canvas character subject", () => {
    test("creates an Image spine with character metadata and cover", () => {
        const node = createCharacterSubjectNode(payload, { x: 100, y: 80 });
        expect(node.type).toBe(CanvasNodeType.Image);
        expect(node.width).toBe(320);
        expect(node.height).toBe(260);
        expect(node.metadata?.workflowKind).toBe("character");
        expect(node.metadata?.characterAssetId).toBe("asset-1");
        expect(node.metadata?.characterCoverUrl).toBe(payload.coverUrl);
        expect(node.metadata?.content).toBe(payload.coverUrl);
        expect(isCanvasCharacterSubject(node)).toBe(true);
    });

    test("finds an existing subject by characterAssetId regardless of node type", () => {
        const existing: CanvasNodeData = {
            id: "old-card",
            type: CanvasNodeType.Text,
            title: "林夏",
            position: { x: 0, y: 0 },
            width: 320,
            height: 260,
            metadata: { workflowKind: "character", characterAssetId: "asset-1" },
        };
        expect(findCharacterSubjectNode([existing], "asset-1")?.id).toBe("old-card");
        expect(findCharacterSubjectNode([existing], "asset-2")).toBeUndefined();
    });
});
