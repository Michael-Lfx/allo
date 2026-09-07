import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import { resolveStoryboardGenerationContext } from "./canvas-storyboard-context";

function node(partial: Partial<CanvasNodeData> & Pick<CanvasNodeData, "id" | "type" | "title">): CanvasNodeData {
    return {
        position: { x: 0, y: 0 },
        width: 160,
        height: 160,
        ...partial,
    };
}

describe("resolveStoryboardGenerationContext", () => {
    test("does not require a look / styleboard", () => {
        const context = resolveStoryboardGenerationContext([
            node({
                id: "char-1",
                type: CanvasNodeType.Image,
                title: "噜噜",
                metadata: { workflowKind: "character", characterName: "噜噜" },
            }),
        ]);
        expect(context.projectStyle.prompt).toBe("");
        expect(context.characters).toEqual([
            expect.objectContaining({ assetId: "", versionId: "", name: "噜噜" }),
        ]);
    });

    test("still requires a synced Yingce character version", () => {
        expect(() => resolveStoryboardGenerationContext([
            node({
                id: "char-1",
                type: CanvasNodeType.Image,
                title: "林夏",
                metadata: { workflowKind: "character", characterAssetId: "asset-1", characterName: "林夏" },
            }),
        ])).toThrow(/版本未同步/);
    });

    test("includes synced Yingce cards and skips unnamed character-design images", () => {
        const context = resolveStoryboardGenerationContext([
            node({
                id: "yingce",
                type: CanvasNodeType.Image,
                title: "林夏",
                metadata: {
                    workflowKind: "character",
                    characterAssetId: "asset-1",
                    characterVersionId: "v1",
                    characterName: "林夏",
                    characterPrompt: "短发",
                },
            }),
            node({
                id: "design",
                type: CanvasNodeType.Image,
                title: "角色设计",
                metadata: { workflowKind: "character" },
            }),
        ]);
        expect(context.characters).toHaveLength(1);
        expect(context.characters[0]).toEqual({
            assetId: "asset-1",
            versionId: "v1",
            name: "林夏",
            definition: { prompt: "短发" },
        });
    });
});
