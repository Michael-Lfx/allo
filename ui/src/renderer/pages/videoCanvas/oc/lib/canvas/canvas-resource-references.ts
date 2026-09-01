import { imageReferenceLabel } from "@oc/lib/image-reference-prompt";
import { seedanceReferenceLabel } from "@oc/lib/seedance-video";
import { canvasNodeVideoPreviewUrl } from "@oc/lib/canvas/canvas-media-preview";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import { skillFromCanvasNode } from "@oc/lib/canvas/canvas-skill-mentions";
import type { Skill } from "@oc/services/api/skills";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";

export type CanvasResourceKind = "image" | "video" | "audio" | "text" | "skill" | "character";

export type CanvasResourceReference = {
    id: string;
    nodeId: string;
    kind: CanvasResourceKind;
    label: string;
    title: string;
    previewUrl?: string;
    storageKey?: string;
    previewStorageKey?: string;
    text?: string;
    active: boolean;
    sourceType?: CanvasNodeType;
    skill?: Skill;
};

export function canvasResourceMentionToken(reference: CanvasResourceReference) {
    if (reference.kind === "skill" && reference.skill?.skill_id) return `@[skill:${reference.skill.skill_id}]`;
    return `@[node:${reference.nodeId}]`;
}

export function buildCanvasResourceReferences(nodes: CanvasNodeData[], connections: CanvasConnection[], contextNodeId?: string | null) {
    const contextNodes = contextNodeId ? getMentionResourceNodes(contextNodeId, nodes, connections) : [];
    const globalReferences = labelResourceNodes(nodes.filter(isResourceNode), false);
    const activeByNodeId = new Map(labelResourceNodes(contextNodes, true).map((reference) => [reference.nodeId, reference]));
    return globalReferences.map((reference) => activeByNodeId.get(reference.nodeId) || reference);
}

export function buildNodeMentionReferences(node: CanvasNodeData, nodes: CanvasNodeData[], connections: CanvasConnection[]) {
    return labelResourceNodes(getMentionResourceNodes(node.id, nodes, connections), true);
}

/**
 * 引用数组的结构签名：内容未变时允许调用方复用旧数组身份，
 * 避免每次语义变化都让全部节点的 props 失去 React.memo 资格。
 */
export function canvasResourceReferencesSignature(references: CanvasResourceReference[]) {
    return references
        .map((reference) => [reference.id, reference.kind, reference.label, reference.title, reference.previewUrl ?? "", reference.storageKey ?? "", reference.previewStorageKey ?? "", reference.text ?? "", reference.active ? "1" : "0", reference.sourceType ?? "", reference.skill?.skill_id ?? reference.skill?.skill_name ?? ""].join("|"))
        .join("\n");
}

export function getMentionResourceNodes(nodeId: string, nodes: CanvasNodeData[], connections: CanvasConnection[]) {
    const configInputs = getConnectedConfigResourceNodes(nodeId, nodes, connections);
    if (configInputs.length) return configInputs;
    const ownInputs = getContextResourceNodes(nodeId, nodes, connections);
    if (ownInputs.length) return ownInputs;
    const node = nodes.find((item) => item.id === nodeId);
    return node && isResourceNode(node) ? [node] : [];
}

export function getGenerationResourceNodes(nodeId: string, nodes: CanvasNodeData[], connections: CanvasConnection[]) {
    const configInputs = getConnectedConfigResourceNodes(nodeId, nodes, connections);
    if (configInputs.length) return configInputs;
    const ownInputs = getContextResourceNodes(nodeId, nodes, connections);
    if (ownInputs.length) return ownInputs;
    return [];
}

export function getContextResourceNodes(nodeId: string, nodes: CanvasNodeData[], connections: CanvasConnection[]) {
    return connections
        .filter((connection) => connection.toNodeId === nodeId)
        .map((connection) => nodes.find((node) => node.id === connection.fromNodeId))
        .filter((node): node is CanvasNodeData => Boolean(node && isResourceNode(node)));
}

function getConnectedConfigResourceNodes(nodeId: string, nodes: CanvasNodeData[], connections: CanvasConnection[]) {
    const configConnection = connections.find((connection) => connection.fromNodeId === nodeId && nodes.find((node) => node.id === connection.toNodeId)?.type === CanvasNodeType.Config);
    if (!configConnection) return [];
    return getContextResourceNodes(configConnection.toNodeId, nodes, connections).filter((node) => node.id !== nodeId);
}

function labelResourceNodes(nodes: CanvasNodeData[], active: boolean) {
    const counts: Record<CanvasResourceKind, number> = { image: 0, video: 0, audio: 0, text: 0, skill: 0, character: 0 };
    let drawingCount = 0;
    return nodes.flatMap((node): CanvasResourceReference[] => {
        const kind = resourceKind(node);
        if (!kind) return [];
        const index = node.type === CanvasNodeType.Drawing ? drawingCount++ : counts[kind]++;
        const label = node.type === CanvasNodeType.Drawing ? `绘图${index + 1}` : labelForKind(kind, index);
        const skill = kind === "skill" ? skillFromCanvasNode(node) || undefined : undefined;
        return [
            {
                id: skill ? `skill:${skill.skill_id}` : node.id,
                nodeId: node.id,
                kind,
                label: skill?.skill_name || label,
                title: skill?.skill_name || node.title || label,
                previewUrl: node.metadata?.workflowKind === "character"
                    ? node.metadata.characterCoverUrl
                    : node.type === CanvasNodeType.Drawing
                        ? node.metadata?.drawingPreviewUrl
                        : node.type === CanvasNodeType.Video
                            ? canvasNodeVideoPreviewUrl(node)
                            : node.metadata?.previewContent || node.metadata?.content,
                storageKey: node.metadata?.storageKey,
                previewStorageKey: node.type === CanvasNodeType.Video ? node.metadata?.videoPreview?.storageKey : undefined,
                text: node.metadata?.workflowKind === "character" ? node.metadata.characterPrompt : node.type === CanvasNodeType.Text ? node.metadata?.content || node.metadata?.prompt : node.type === CanvasNodeType.Skill ? skillResourceText(node) : undefined,
                active,
                sourceType: node.type,
                skill,
            },
        ];
    });
}

function labelForKind(kind: CanvasResourceKind, index: number) {
    if (kind === "character") return `角色${index + 1}`;
    if (kind === "image") return imageReferenceLabel(index);
    if (kind === "video") return seedanceReferenceLabel("video", index);
    if (kind === "audio") return seedanceReferenceLabel("audio", index);
    if (kind === "skill") return `技能${index + 1}`;
    return `文本${index + 1}`;
}

function isResourceNode(node: CanvasNodeData) {
    return Boolean(resourceKind(node));
}

function resourceKind(node: CanvasNodeData): CanvasResourceKind | null {
    // 角色卡是跨类型覆盖：落在可引用类型上时优先记为角色。
    if (node.metadata?.workflowKind === "character" && node.metadata.characterAssetId) return "character";
    return getNodeResourceKind(node);
}

function skillResourceText(node: CanvasNodeData) {
    const skill = node.metadata?.skillSnapshot;
    if (!skill) return node.metadata?.content || "";
    return [skill.name, skill.description, skill.template, skill.outputContract].filter(Boolean).join("\n\n");
}
