import { buildCanvasAgentPlan } from "./canvas-agent-plan";
import { buildCanvasAgentAliasMap, canvasAgentShortId, resolveCanvasAgentNodeId, resolveCanvasAgentNodeIds } from "./canvas-agent-ids";
import { videoEditOperationForKeyframeCount } from "@oc/services/api/video-reference-roles";
import { buildCanvasWorkflowOps, prefixCanvasNodeMentions, type CanvasWorkflowInput, type CanvasWorkflowNodeInput } from "./canvas-agent-workflow";
import { findCanvasAgentNodes, getCanvasAgentNode, getCanvasAgentResources } from "./canvas-agent-context";
import {
    buildCanvasAgentObservation,
    CANVAS_AGENT_CODES,
    generationModeForNode,
    type CanvasAgentObservation,
} from "./canvas-agent-observation";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "./canvas-agent-ops";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { AiConfig } from "@oc/stores/use-config-store";

export type CanvasApplyPatch = {
    id: string;
    title?: string;
    content?: string;
    prompt?: string;
    seconds?: string;
    metadata?: Record<string, unknown>;
};

export type CanvasApplyInput = Omit<CanvasWorkflowInput, "nodes"> & {
    nodes?: CanvasWorkflowNodeInput[];
    patches?: CanvasApplyPatch[];
    deleteIds?: string[];
    run?: boolean;
};

export type CanvasRepairInput = {
    action?: "rerun" | "patch" | "rewire_refs";
    nodeIds?: string[];
    patches?: CanvasApplyPatch[];
    edges?: Array<{ from: string; to: string }>;
};

export const APPLY_NEEDS_GRAPH_MESSAGE = "canvas_apply 必须带 nodes（以及 edges），或用 patches / deleteIds 改已有节点。只传 description 不会改画布；请按用户目标设计节点后一次性写入，不要先空跑。";

export function canvasApplyHasMutation(input: Pick<CanvasApplyInput, "nodes" | "patches" | "edges" | "deleteIds">) {
    return Boolean(
        (Array.isArray(input.nodes) && input.nodes.length) ||
        (Array.isArray(input.patches) && input.patches.length) ||
        (Array.isArray(input.edges) && input.edges.length) ||
        (Array.isArray(input.deleteIds) && input.deleteIds.length),
    );
}

export function isCanvasApplyNeedsGraphError(error: unknown) {
    const message = error instanceof Error ? error.message : String(error || "");
    return message === APPLY_NEEDS_GRAPH_MESSAGE || message.includes("必须带 nodes");
}

export function compileCanvasApplyOps(input: CanvasApplyInput, snapshot: CanvasAgentSnapshot, config: AiConfig): CanvasAgentOp[] {
    const ops: CanvasAgentOp[] = [];
    const deleteIds = resolveCanvasAgentNodeIds(snapshot, input.deleteIds || []).ids;
    if (deleteIds.length) ops.push({ type: "delete_node", ids: deleteIds });
    for (const patch of input.patches || []) {
        ops.push(patchToOp(snapshot, patch));
    }
    if (Array.isArray(input.nodes) && input.nodes.length) {
        const workflow = buildCanvasWorkflowOps({
            title: input.title,
            description: input.description,
            nodes: input.nodes,
            edges: input.edges,
            direction: input.direction,
            start: input.start,
            gap: input.gap,
            autoRun: input.run === true || input.autoRun === true || input.nodes.some((node) => node.runGeneration),
        }, snapshot, config);
        ops.push(...workflow);
    } else if (input.edges?.length) {
        for (const edge of input.edges) {
            const fromNodeId = resolveCanvasAgentNodeId(snapshot, edge.from);
            const toNodeId = resolveCanvasAgentNodeId(snapshot, edge.to);
            if (!fromNodeId || !toNodeId) throw new Error(`连线引用不存在：${edge.from} → ${edge.to}`);
            ops.push({ type: "connect_nodes", fromNodeId, toNodeId });
        }
    }
    if (!ops.length && (input.run === true || input.autoRun === true)) {
        try {
            return compileCanvasRunOps(snapshot);
        } catch {
            throw new Error(APPLY_NEEDS_GRAPH_MESSAGE);
        }
    }
    if (!ops.length) throw new Error(APPLY_NEEDS_GRAPH_MESSAGE);
    return ops;
}

export function compileCanvasRunOps(snapshot: CanvasAgentSnapshot, nodeIds?: string[]): CanvasAgentOp[] {
    const targets = resolveRunTargets(snapshot, nodeIds);
    if (!targets.length) throw new Error("没有可生成的节点。请指定 nodeIds，或先 apply 带 prompt 的图片/视频节点。");
    return targets.map((node) => ({
        type: "run_generation" as const,
        nodeId: node.id,
        mode: generationModeForNode(node),
        prompt: String(node.metadata?.prompt || node.metadata?.composerContent || "").trim() || undefined,
    }));
}

export function compileCanvasRepairOps(input: CanvasRepairInput, snapshot: CanvasAgentSnapshot): CanvasAgentOp[] {
    const action = input.action || inferRepairAction(input);
    const ops: CanvasAgentOp[] = [];
    if (action === "patch" || input.patches?.length) {
        for (const patch of input.patches || []) ops.push(patchToOp(snapshot, patch));
    }
    if (action === "rewire_refs" || (!input.action && !input.patches?.length && !input.nodeIds?.length)) {
        ops.push(...rewireReferenceOps(snapshot, input.nodeIds));
    }
    if (input.edges?.length) {
        for (const edge of input.edges) {
            const fromNodeId = resolveCanvasAgentNodeId(snapshot, edge.from);
            const toNodeId = resolveCanvasAgentNodeId(snapshot, edge.to);
            if (!fromNodeId || !toNodeId) throw new Error(`连线引用不存在：${edge.from} → ${edge.to}`);
            ops.push({ type: "connect_nodes", fromNodeId, toNodeId });
        }
    }
    if (action === "rerun") {
        const targets = resolveRepairRerunTargets(snapshot, input.nodeIds);
        ops.push(...targets.map((node) => ({
            type: "run_generation" as const,
            nodeId: node.id,
            mode: generationModeForNode(node),
            prompt: String(node.metadata?.prompt || node.metadata?.composerContent || "").trim() || undefined,
        })));
    }
    if (!ops.length) throw new Error("repair 没有可执行的变更。可指定 action=rerun|patch|rewire_refs。");
    return ops;
}

export function proposeCanvasApply(input: CanvasApplyInput, snapshot: CanvasAgentSnapshot, config: AiConfig) {
    const ops = compileCanvasApplyOps(input, snapshot, config);
    const plan = buildCanvasAgentPlan(ops, snapshot, config);
    return {
        ok: true,
        code: CANVAS_AGENT_CODES.OK,
        dryRun: true,
        createdEstimate: ops.filter((op) => op.type === "add_node").length,
        edgeEstimate: ops.filter((op) => op.type === "connect_nodes").length,
        generationEstimate: ops.filter((op) => op.type === "run_generation").length,
        plan: {
            title: plan.title,
            stages: plan.stages,
            models: plan.models,
            spend: plan.spend,
            warning: plan.warning,
            items: plan.items,
        },
    };
}

export function inspectCanvasIntent(snapshot: CanvasAgentSnapshot, args: Record<string, unknown>, previous?: CanvasAgentSnapshot | null) {
    const observation = buildCanvasAgentObservation(snapshot, previous);
    const ids = Array.isArray(args.ids) ? args.ids.filter((id): id is string => typeof id === "string") : [];
    const query = typeof args.query === "string" ? args.query : "";
    const types = Array.isArray(args.types) ? args.types.filter((item): item is string => typeof item === "string") : undefined;
    if (ids.length === 1 && !query) {
        return { observation, node: getCanvasAgentNode(snapshot, { id: ids[0] }) };
    }
    if (query || ids.length || types?.length) {
        return { observation, ...findCanvasAgentNodes(snapshot, { query, ids, types, limit: typeof args.limit === "number" ? args.limit : 30 }) };
    }
    if (args.focus === "resources") {
        return { observation, ...getCanvasAgentResources(snapshot, { limit: 50 }) };
    }
    return {
        observation,
        graph: summarizeGraph(snapshot, observation),
    };
}

export function critiqueCanvasOutputs(snapshot: CanvasAgentSnapshot, nodeIds?: string[]) {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    const targets = (nodeIds?.length ? resolveCanvasAgentNodeIds(snapshot, nodeIds).ids : snapshot.nodes.filter(isMediaOrGenerated).map((node) => node.id));
    const issues: Array<{ id: string; shortId: string; code: string; message: string }> = [];
    const nodes = targets.map((id) => {
        const node = snapshot.nodes.find((item) => item.id === id);
        if (!node) {
            issues.push({ id, shortId: id, code: CANVAS_AGENT_CODES.MISSING_REF, message: "节点不存在" });
            return null;
        }
        const inbound = snapshot.connections.filter((connection) => connection.toNodeId === node.id);
        const inboundImages = inbound
            .map((connection) => snapshot.nodes.find((item) => item.id === connection.fromNodeId))
            .filter((item): item is CanvasNodeData => item?.type === CanvasNodeType.Image);
        const prompt = String(node.metadata?.prompt || node.metadata?.composerContent || "");
        const status = String(node.metadata?.status || "idle");
        const ready = status === "success" && Boolean(node.metadata?.storageKey || node.metadata?.primaryImageId || node.metadata?.resourceId);
        if (FAILED_STATUS.has(status)) issues.push({ id: node.id, shortId: canvasAgentShortId(node.id, aliases), code: CANVAS_AGENT_CODES.GENERATION_FAILED, message: String(node.metadata?.errorDetails || "生成失败") });
        if (node.type === CanvasNodeType.Video && inboundImages.length) {
            const missingMentions = inboundImages.filter((image) => !prompt.includes(`@[node:${image.id}]`));
            if (missingMentions.length) {
                issues.push({
                    id: node.id,
                    shortId: canvasAgentShortId(node.id, aliases),
                    code: CANVAS_AGENT_CODES.MISSING_REF,
                    message: `视频提示词未 @ 全部入边关键帧：${missingMentions.map((image) => canvasAgentShortId(image.id, aliases)).join(", ")}。调用 canvas_repair action=rewire_refs。`,
                });
            }
            const emptyUpstream = inboundImages.filter((image) => String(image.metadata?.status) !== "success");
            if (emptyUpstream.length) {
                issues.push({
                    id: node.id,
                    shortId: canvasAgentShortId(node.id, aliases),
                    code: CANVAS_AGENT_CODES.UPSTREAM_EMPTY,
                    message: `上游关键帧尚未就绪：${emptyUpstream.map((image) => canvasAgentShortId(image.id, aliases)).join(", ")}`,
                });
            }
        }
        if (generationModeForNode(node) && status === "idle" && !(prompt || node.metadata?.content)) {
            issues.push({ id: node.id, shortId: canvasAgentShortId(node.id, aliases), code: CANVAS_AGENT_CODES.NOOP, message: "节点缺少 prompt/content，无法生成" });
        }
        return {
            id: node.id,
            shortId: canvasAgentShortId(node.id, aliases),
            type: node.type,
            title: node.title,
            status,
            ready,
            prompt: prompt.slice(0, 240),
            inbound: inbound.map((connection) => ({
                from: canvasAgentShortId(connection.fromNodeId, aliases),
                handle: connection.fromHandleId,
            })),
            startFrame: node.metadata?.videoStartFrameNodeId,
            endFrame: node.metadata?.videoEndFrameNodeId,
        };
    });
    return {
        ok: issues.length === 0,
        code: issues[0]?.code || CANVAS_AGENT_CODES.OK,
        nodes: nodes.filter(Boolean),
        issues,
    };
}

export function resolveRunTargets(snapshot: CanvasAgentSnapshot, nodeIds?: string[]) {
    if (nodeIds?.length) {
        const resolved = resolveCanvasAgentNodeIds(snapshot, nodeIds);
        if (resolved.missing.length) throw new Error(`找不到节点：${resolved.missing.join(", ")}`);
        return resolved.ids.map((id) => snapshot.nodes.find((node) => node.id === id)!).filter(Boolean);
    }
    return snapshot.nodes.filter((node) => {
        const mode = generationModeForNode(node);
        if (!mode) return false;
        const status = String(node.metadata?.status || "idle");
        if (status === "success" && (node.metadata?.storageKey || node.metadata?.primaryImageId)) return false;
        if (["pending", "loading", "queued", "running", "processing"].includes(status)) return false;
        return Boolean(String(node.metadata?.prompt || node.metadata?.composerContent || node.metadata?.content || "").trim());
    });
}

function rewireReferenceOps(snapshot: CanvasAgentSnapshot, nodeIds?: string[]): CanvasAgentOp[] {
    const videos = snapshot.nodes.filter((node) => {
        if (node.type !== CanvasNodeType.Video) return false;
        if (!nodeIds?.length) return true;
        return resolveCanvasAgentNodeIds(snapshot, nodeIds).ids.includes(node.id);
    });
    const ops: CanvasAgentOp[] = [];
    for (const video of videos) {
        const inboundImages = snapshot.connections
            .filter((connection) => connection.toNodeId === video.id)
            .map((connection) => snapshot.nodes.find((node) => node.id === connection.fromNodeId))
            .filter((node): node is CanvasNodeData => node?.type === CanvasNodeType.Image);
        if (!inboundImages.length) continue;
        const inboundIds = inboundImages.map((node) => node.id);
        const prompt = prefixCanvasNodeMentions(String(video.metadata?.prompt || video.metadata?.composerContent || ""), inboundIds);
        ops.push({
            type: "update_node",
            id: video.id,
            metadata: {
                prompt,
                composerContent: prompt,
                videoEditOperation: videoEditOperationForKeyframeCount(inboundIds.length),
                videoStartFrameNodeId: inboundIds[0],
                videoEndFrameNodeId: inboundIds[inboundIds.length - 1],
                referenceNodeIds: inboundIds,
            },
        });
    }
    return ops;
}

function resolveRepairRerunTargets(snapshot: CanvasAgentSnapshot, nodeIds?: string[]) {
    if (nodeIds?.length) return resolveRunTargets(snapshot, nodeIds);
    return snapshot.nodes.filter((node) => {
        const status = String(node.metadata?.status || "idle");
        return Boolean(generationModeForNode(node)) && (status === "error" || status === "failed" || status === "idle");
    });
}

function inferRepairAction(input: CanvasRepairInput) {
    if (input.patches?.length) return "patch";
    if (input.nodeIds?.length) return "rerun";
    return "rewire_refs";
}

function patchToOp(snapshot: CanvasAgentSnapshot, patch: CanvasApplyPatch): CanvasAgentOp {
    const id = resolveCanvasAgentNodeId(snapshot, patch.id);
    if (!id) throw new Error(`找不到要修补的节点：${patch.id}`);
    return {
        type: "update_node",
        id,
        patch: patch.title ? { title: patch.title } : undefined,
        metadata: {
            ...(patch.content !== undefined ? { content: patch.content } : {}),
            ...(patch.prompt !== undefined ? { prompt: patch.prompt, composerContent: patch.prompt } : {}),
            ...(patch.seconds !== undefined ? { seconds: patch.seconds } : {}),
            ...(patch.metadata || {}),
        },
    };
}

function summarizeGraph(snapshot: CanvasAgentSnapshot, observation: CanvasAgentObservation) {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    return {
        nodes: snapshot.nodes.map((node) => ({
            id: node.id,
            shortId: canvasAgentShortId(node.id, aliases),
            type: node.type,
            title: node.title,
            status: node.metadata?.status || "idle",
            prompt: String(node.metadata?.prompt || node.metadata?.composerContent || "").slice(0, 180),
        })),
        connections: snapshot.connections.map((connection) => ({
            from: canvasAgentShortId(connection.fromNodeId, aliases),
            to: canvasAgentShortId(connection.toNodeId, aliases),
            handle: connection.fromHandleId,
        })),
        observation,
    };
}

function isMediaOrGenerated(node: CanvasNodeData) {
    return Boolean(generationModeForNode(node)) || node.type === CanvasNodeType.Image || node.type === CanvasNodeType.Video || node.type === CanvasNodeType.Audio;
}

const FAILED_STATUS = new Set(["error", "failed", "cancelled", "canceled"]);

export type { CanvasWorkflowNodeInput };
