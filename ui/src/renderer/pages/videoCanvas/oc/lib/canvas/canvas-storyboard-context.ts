import type { CanvasNodeData } from "@oc/types/canvas";

export type StoryboardGenerationContext = {
    projectStyle: {
        presetId: string;
        title: string;
        prompt: string;
    };
    characters: Array<{
        assetId: string;
        versionId: string;
        name: string;
        definition: Record<string, unknown>;
    }>;
};

/**
 * Shared context for storyboard row generation.
 * Look / styleboard is optional (creation mode may have no look).
 * Yingce character cards still require a synced version; labeled creation
 * subjects (`workflowKind=character` without asset ids) are included by name.
 */
export function resolveStoryboardGenerationContext(nodes: CanvasNodeData[]): StoryboardGenerationContext {
    const styleNode = nodes.find((node) => node.metadata?.workflowKind === "styleboard");
    const stylePrompt = String(styleNode?.metadata?.content || styleNode?.metadata?.prompt || "").trim();
    const stylePresetId = String(styleNode?.metadata?.stylePresetId || "").trim();

    const yingceCards = nodes.filter((node) => node.metadata?.workflowKind === "character" && node.metadata?.characterAssetId?.trim());
    const invalidCharacter = yingceCards.find((node) => !node.metadata?.characterVersionId?.trim() || !(node.metadata?.characterName || node.title).trim());
    if (invalidCharacter) throw new Error(`角色卡“${invalidCharacter.metadata?.characterName || invalidCharacter.title || "未命名角色"}”版本未同步，请刷新角色资产后再生成分镜`);

    const labeledSubjects = nodes.filter((node) => node.metadata?.workflowKind === "character" && !node.metadata?.characterAssetId?.trim() && (node.metadata?.characterName || "").trim());
    const characterNodes = [...yingceCards, ...labeledSubjects];
    const styleTitle = styleNode
        ? styleNode.title.replace(/^(?:项目)?画风\s*[·：:]?\s*/, "").trim() || styleNode.title
        : "";

    return {
        projectStyle: {
            presetId: stylePresetId,
            title: styleTitle,
            prompt: stylePrompt,
        },
        characters: characterNodes.map((node) => ({
            assetId: node.metadata?.characterAssetId?.trim() || "",
            versionId: node.metadata?.characterVersionId?.trim() || "",
            name: (node.metadata?.characterName || node.title).trim(),
            definition: node.metadata?.characterDefinition || (
                node.metadata?.characterAssetId?.trim()
                    ? { prompt: node.metadata?.characterPrompt || "" }
                    : { prompt: node.metadata?.characterPrompt || "", mediaNodeId: node.id }
            ),
        })),
    };
}
