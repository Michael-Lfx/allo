import { buildCanvasAgentAliasMap, canvasAgentShortId } from "@oc/lib/canvas/canvas-agent-ids";
import type { CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import type { CanvasNodeData } from "@oc/types/canvas";

/**
 * 压缩画布快照用于 Agent 聊天记录 / 事件日志等长驻内存的场景：
 * 剥离媒体数据（base64/blob/dataUrl）与长文本，仅保留诊断所需字段。
 * 完整快照仍通过 postToolResult / postState 发送给 Agent 本体。
 */
const MAX_SCENE_NODES = 48;

export function compactCanvasAgentSnapshot(snapshot: CanvasAgentSnapshot) {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    return {
        projectId: snapshot.projectId,
        title: snapshot.title,
        viewport: snapshot.viewport,
        selectedNodeIds: snapshot.selectedNodeIds,
        nodes: snapshot.nodes.map((node) => ({
            id: node.id,
            shortId: canvasAgentShortId(node.id, aliases),
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
        agentAlias: metadata?.agentAlias,
        taskId: metadata?.taskId,
        storyboardRowCount: Array.isArray(metadata?.storyboard?.rows) ? metadata.storyboard.rows.length : undefined,
        videoStartFrameNodeId: metadata?.videoStartFrameNodeId,
        videoEndFrameNodeId: metadata?.videoEndFrameNodeId,
        characterName: metadata?.characterName,
        characterAssetId: metadata?.characterAssetId,
        characterVersionId: metadata?.characterVersionId,
        chapterId: metadata?.chapterId,
        chapterTitle: metadata?.chapterTitle,
        shotIndex: metadata?.shotIndex,
    };
}

export function formatCanvasAgentScene(snapshot: CanvasAgentSnapshot) {
    const compact = compactCanvasAgentSnapshot(snapshot);
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    const selected = (compact.selectedNodeIds || []).map((id) => canvasAgentShortId(id, aliases)).join(",") || "none";
    const visible = compact.nodes.slice(0, MAX_SCENE_NODES);
    const extra = compact.nodes.length - visible.length;
    const nodeLines = visible.map((node) => {
        const x = Math.round(node.position?.x ?? 0);
        const y = Math.round(node.position?.y ?? 0);
        const width = Math.round(node.width ?? 0);
        const height = Math.round(node.height ?? 0);
        const status = node.metadata?.status ? ` ${node.metadata.status}` : "";
        return `${node.shortId} ${node.type} "${String(node.title || "").slice(0, 40)}" @${x},${y} ${width}x${height}${status}`;
    });
    const edgeLine = compact.connections.length
        ? compact.connections.map((connection) => `${canvasAgentShortId(connection.fromNodeId, aliases)}->${canvasAgentShortId(connection.toNodeId, aliases)}`).join(", ")
        : "none";
    return [
        `scene nodes=${compact.nodes.length} edges=${compact.connections.length} selected=${selected}`,
        ...nodeLines,
        extra > 0 ? `…and ${extra} more nodes` : "",
        `edges ${edgeLine}`,
    ].filter(Boolean).join("\n");
}

