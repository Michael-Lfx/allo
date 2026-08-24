import type { CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import type { CanvasNodeData } from "@oc/types/canvas";

/**
 * 压缩画布快照用于 Agent 聊天记录 / 事件日志等长驻内存的场景：
 * 剥离媒体数据（base64/blob/dataUrl）与长文本，仅保留诊断所需字段。
 * 完整快照仍通过 postToolResult / postState 发送给 Agent 本体。
 */
export function compactCanvasAgentSnapshot(snapshot: CanvasAgentSnapshot) {
    return {
        projectId: snapshot.projectId,
        title: snapshot.title,
        viewport: snapshot.viewport,
        selectedNodeIds: snapshot.selectedNodeIds,
        nodes: snapshot.nodes.map((node) => ({
            id: node.id,
            type: node.type,
            title: node.title,
            position: node.position,
            width: node.width,
            height: node.height,
            metadata: compactCanvasNodeMetadata(node.metadata || {}),
        })),
        connections: snapshot.connections,
    };
}

export function compactCanvasNodeMetadata(metadata: CanvasNodeData["metadata"]) {
    return {
        content: String(metadata?.content || "").slice(0, 500),
        prompt: String(metadata?.prompt || metadata?.composerContent || "").slice(0, 500),
        status: metadata?.status,
        skillName: metadata?.skillSnapshot?.name,
        skillVersion: metadata?.skillSnapshot?.version,
        generationMode: metadata?.generationMode,
        model: metadata?.model,
        size: metadata?.size,
        assetTags: metadata?.assetTags,
        workflowKind: metadata?.workflowKind,
        workflowTitle: metadata?.workflowTitle,
        workflowDescription: metadata?.workflowDescription,
        characterName: metadata?.characterName,
        characterAssetId: metadata?.characterAssetId,
        characterVersionId: metadata?.characterVersionId,
        chapterId: metadata?.chapterId,
        chapterTitle: metadata?.chapterTitle,
        shotIndex: metadata?.shotIndex,
    };
}

