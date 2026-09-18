import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import type { CanvasAgentOp } from "@oc/lib/canvas/canvas-agent-ops";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type CanvasNodeMetadata, type Position } from "@oc/types/canvas";

import type { CompiledAsset, CompiledGenerationTemplate } from "./types";

export type TemplateApplyGraph = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
};

export type MaterializeGenerationTemplateInput = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    selectedNode?: CanvasNodeData | null;
    canvasCenter: Position;
};

export type MaterializedGenerationTemplate = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    target: CanvasNodeData;
    createdNodeIds: string[];
    selectedNodeIds: string[];
};

function mention(id: string) {
    return `@[node:${id}]`;
}

function withMentions(prompt: string, ids: string[]) {
    return ids.reduce((text, id) => (text.includes(mention(id)) ? text : `${text}\n${mention(id)}`.trim()), prompt);
}

function assetNode(asset: CompiledAsset, center: Position, title: string): CanvasNodeData {
    const node = createCanvasNode(CanvasNodeType.Image, center, {
        content: asset.url,
        previewContent: asset.url,
        prompt: title,
        status: "success",
        generationMode: "image",
    });
    node.title = title;
    return node;
}

function assetTitle(asset: CompiledAsset): string {
    if (asset.role === "start_frame") return "Start frame";
    if (asset.role === "end_frame") return "End frame";
    if (asset.role === "character") return "Character ref";
    if (asset.role === "style") return "Style ref";
    return "Motion ref";
}

function link(from: CanvasNodeData, to: CanvasNodeData): CanvasConnection {
    return { id: `edge-${from.id}-${to.id}`, fromNodeId: from.id, toNodeId: to.id };
}

function nodeCenter(node: CanvasNodeData): Position {
    return { x: node.position.x + node.width / 2, y: node.position.y + node.height / 2 };
}

function createTarget(compiled: CompiledGenerationTemplate, center: Position): CanvasNodeData {
    const spec = NODE_DEFAULT_SIZE[compiled.nodeType];
    const node = createCanvasNode(compiled.nodeType, center, targetMetadata(compiled, undefined, []));
    node.title = compiled.template.title;
    node.width = spec.width;
    node.height = spec.height;
    return node;
}

function targetMetadata(compiled: CompiledGenerationTemplate, previous: CanvasNodeMetadata | undefined, mentionIds: string[]): CanvasNodeMetadata {
    const policy = compiled.applyPolicy;
    const nextPrompt = withMentions(compiled.prompt, mentionIds);
    const keepExisting = policy === "merge";
    const slotOnly = policy === "slot-fill";
    const prompt = slotOnly ? nextPrompt : keepExisting && previous?.composerContent?.trim() ? previous.composerContent : nextPrompt;
    const model = slotOnly ? previous?.model : compiled.model || (keepExisting ? previous?.model : compiled.model);
    const videoEditOperation = compiled.nodeType === CanvasNodeType.Video && !slotOnly ? compiled.operation : previous?.videoEditOperation;
    const seconds = slotOnly ? previous?.seconds : compiled.seconds || (keepExisting ? previous?.seconds : compiled.seconds);
    const size = slotOnly ? previous?.size : compiled.size || (keepExisting ? previous?.size : compiled.size);
    return {
        ...previous,
        composerContent: prompt,
        prompt,
        model,
        seconds,
        size,
        generationMode: compiled.nodeType === CanvasNodeType.Video ? "video" : "image",
        videoEditOperation: compiled.nodeType === CanvasNodeType.Video ? videoEditOperation : previous?.videoEditOperation,
        promptTemplateVariables: compiled.slotValues,
        appliedTemplate: compiled.stamp,
    };
}

function patchFrameIds(metadata: CanvasNodeMetadata, compiled: CompiledGenerationTemplate, startId?: string, endId?: string): CanvasNodeMetadata {
    if (compiled.applyPolicy === "slot-fill" || compiled.nodeType !== CanvasNodeType.Video) return metadata;
    if (compiled.operation === "image_to_video") {
        return { ...metadata, videoStartFrameNodeId: startId, videoEndFrameNodeId: endId, videoEditOperation: "image_to_video" };
    }
    if (compiled.operation === "reference_to_video") {
        return { ...metadata, videoStartFrameNodeId: undefined, videoEndFrameNodeId: undefined, videoEditOperation: "reference_to_video" };
    }
    return { ...metadata, videoStartFrameNodeId: undefined, videoEndFrameNodeId: undefined, videoEditOperation: "text_to_video" };
}

export function materializeGenerationTemplate(compiled: CompiledGenerationTemplate, input: MaterializeGenerationTemplateInput): MaterializedGenerationTemplate {
    const selected = input.selectedNode && (input.selectedNode.type === compiled.nodeType || compiled.applyPolicy === "slot-fill") ? input.selectedNode : undefined;
    const created: CanvasNodeData[] = [];
    const addedConnections: CanvasConnection[] = [];
    let nodes = [...input.nodes];
    let connections = [...input.connections];
    const target = selected || createTarget(compiled, input.canvasCenter);
    if (!selected) {
        created.push(target);
        nodes = [...nodes, target];
    }
    const center = nodeCenter(target);
    const gap = NODE_DEFAULT_SIZE[CanvasNodeType.Image].width + 60;
    const place = (asset: CompiledAsset, index: number) => {
        const node = assetNode(asset, { x: center.x - gap * (index + 1), y: center.y }, assetTitle(asset));
        created.push(node);
        nodes = [...nodes, node];
        addedConnections.push(link(node, target));
        return node;
    };

    let startId: string | undefined;
    let endId: string | undefined;
    const mentionIds: string[] = [];
    if (compiled.applyPolicy !== "slot-fill") {
        let index = 0;
        if (compiled.startFrame) {
            const node = place(compiled.startFrame, index++);
            startId = node.id;
            mentionIds.push(node.id);
        }
        if (compiled.endFrame) {
            const node = place(compiled.endFrame, index++);
            endId = node.id;
            mentionIds.push(node.id);
        }
        for (const asset of compiled.referenceAssets) {
            mentionIds.push(place(asset, index++).id);
        }
        if (compiled.applyPolicy === "replace") {
            const keep = new Set(mentionIds);
            connections = connections.filter((item) => {
                if (item.toNodeId !== target.id) return true;
                const from = nodes.find((node) => node.id === item.fromNodeId);
                return from?.type !== CanvasNodeType.Image || keep.has(item.fromNodeId);
            });
        }
        connections = [...connections, ...addedConnections];
    }

    const metadata = patchFrameIds(targetMetadata(compiled, selected?.metadata, mentionIds), compiled, startId, endId);
    const nextTarget: CanvasNodeData = { ...target, metadata };
    nodes = nodes.map((node) => (node.id === target.id ? nextTarget : node));
    return {
        nodes,
        connections,
        target: nextTarget,
        createdNodeIds: created.map((node) => node.id),
        selectedNodeIds: [nextTarget.id],
    };
}

export function templateApplyToOps(plan: MaterializedGenerationTemplate, previous: TemplateApplyGraph): CanvasAgentOp[] {
    const previousIds = new Set(previous.nodes.map((node) => node.id));
    const ops: CanvasAgentOp[] = [];
    for (const node of plan.nodes) {
        if (previousIds.has(node.id)) continue;
        ops.push({
            type: "add_node",
            id: node.id,
            nodeType: node.type,
            title: node.title,
            position: node.position,
            width: node.width,
            height: node.height,
            metadata: node.metadata,
        });
    }
    const previousTarget = previous.nodes.find((node) => node.id === plan.target.id);
    if (previousTarget) {
        ops.push({ type: "update_node", id: plan.target.id, patch: { title: plan.target.title }, metadata: plan.target.metadata });
    }
    const previousEdgeIds = new Set(previous.connections.map((item) => item.id));
    for (const connection of plan.connections) {
        if (previousEdgeIds.has(connection.id)) continue;
        ops.push({ type: "connect_nodes", id: connection.id, fromNodeId: connection.fromNodeId, toNodeId: connection.toNodeId });
    }
    const nextEdgeIds = new Set(plan.connections.map((item) => item.id));
    const removed = previous.connections.filter((item) => item.toNodeId === plan.target.id && !nextEdgeIds.has(item.id)).map((item) => item.id);
    if (removed.length) ops.push({ type: "delete_connections", ids: removed });
    ops.push({ type: "select_nodes", ids: plan.selectedNodeIds });
    return ops;
}
