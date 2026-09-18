import type { InsertAssetPayload } from "@oc/components/canvas/asset-picker-modal";
import { CanvasNodeType, type CanvasNodeData, type CanvasNodeMetadata, type Position } from "@oc/types/canvas";

const CHARACTER_SUBJECT_SIZE = { width: 320, height: 260 } as const;
const NODE_STATUS_SUCCESS = "success" as const;

type CharacterInsertPayload = Extract<InsertAssetPayload, { kind: "character" }>;

export function isCanvasCharacterSubject(node?: Pick<CanvasNodeData, "metadata"> | null): boolean {
    return Boolean(node?.metadata?.workflowKind === "character" && node.metadata.characterAssetId);
}

export function findCharacterSubjectNode(nodes: CanvasNodeData[], assetId: string): CanvasNodeData | undefined {
    const id = assetId.trim();
    if (!id) return undefined;
    return nodes.find((node) => node.metadata?.workflowKind === "character" && node.metadata.characterAssetId === id);
}

/**
 * 角色卡脊柱是 Image + workflowKind=character + characterAssetId。
 * 不再落 Text 孤岛：角色引用渲染走跨类型覆盖，不依赖节点 type。
 */
export function createCharacterSubjectNode(payload: CharacterInsertPayload, center: Position): CanvasNodeData {
    const cover = payload.coverUrl?.trim() || undefined;
    const metadata: CanvasNodeMetadata = {
        workflowKind: "character",
        characterAssetId: payload.assetId,
        characterVersionId: payload.versionId,
        characterVersionPolicy: "current",
        characterName: payload.title,
        characterPrompt: payload.prompt,
        characterAliases: payload.aliases,
        characterDefinition: payload.definition,
        characterCoverUrl: cover,
        characterVisualStatus: payload.visualStatus,
        characterVoiceStatus: payload.voiceStatus,
        characterVoiceName: payload.voiceName,
        characterVoiceProfile: payload.voiceProfile,
        characterVoiceInstructions: payload.voiceInstructions,
        assetId: payload.assetId,
        content: cover,
        status: NODE_STATUS_SUCCESS,
    };
    return {
        id: `character-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
        type: CanvasNodeType.Image,
        title: payload.title,
        position: { x: center.x - CHARACTER_SUBJECT_SIZE.width / 2, y: center.y - CHARACTER_SUBJECT_SIZE.height / 2 },
        width: CHARACTER_SUBJECT_SIZE.width,
        height: CHARACTER_SUBJECT_SIZE.height,
        metadata,
    };
}
