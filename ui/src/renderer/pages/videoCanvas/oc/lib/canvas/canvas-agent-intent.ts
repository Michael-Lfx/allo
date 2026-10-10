import { buildCanvasAgentPlan } from "./canvas-agent-plan";
import { buildCanvasAgentAliasMap, canvasAgentShortId, resolveCanvasAgentNodeId, resolveCanvasAgentNodeIds } from "./canvas-agent-ids";
import { videoEditOperationForKeyframeCount } from "@oc/services/api/video-reference-roles";
import {
    APPLY_ALREADY_SATISFIED_MESSAGE,
    compileCanvasLayoutOps,
    extractPatchGeometry,
    looksLikeCanvasLayoutRequest,
    shouldCompileCanvasApplyAsLayout,
} from "./canvas-agent-layout";
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
import { resolveCreationIr, summarizeCreationForAgent } from "@renderer/pages/videoCanvas/lib/creation-ir";
import { CREATION_INSPECT_FOCUS, summarizeCreationDomain } from "./creation-agent-intent";
import { unchangedModeratedPrompt } from "@oc/lib/generation-error";

export const CANVAS_AGENT_BUSY_RERUN_MESSAGE = "指定节点仍在生成中，不要再次 canvas_run / repair rerun。";
export const CANVAS_AGENT_MODERATION_RERUN_MESSAGE = "内容审核未通过且提示词未改，不要 rerun。请先修改 prompt。";

export type CanvasApplyPatch = {
    id: string;
    title?: string;
    content?: string;
    prompt?: string;
    seconds?: string;
    position?: { x: number; y: number };
    x?: number;
    y?: number;
    width?: number;
    height?: number;
    metadata?: Record<string, unknown>;
};

export type CanvasApplyInput = Omit<CanvasWorkflowInput, "nodes"> & {
    nodes?: CanvasWorkflowNodeInput[];
    patches?: CanvasApplyPatch[];
    deleteIds?: string[];
    nodeIds?: string[];
    layout?: boolean;
    run?: boolean;
};

export type CanvasRepairInput = {
    action?: "rerun" | "patch" | "rewire_refs";
    nodeIds?: string[];
    patches?: CanvasApplyPatch[];
    edges?: Array<{ from: string; to: string }>;
};

export const APPLY_NEEDS_GRAPH_MESSAGE = "canvas_apply 必须带 nodes/edges 新建图，或用 patches.position / layout 移动已有节点，或用 deleteIds 删除。只传 description 且不是整理布局时不会改画布。";

export function canvasApplyHasMutation(input: Pick<CanvasApplyInput, "nodes" | "patches" | "edges" | "deleteIds" | "layout" | "nodeIds" | "description">) {
    return Boolean(
        input.layout === true ||
        looksLikeCanvasLayoutRequest(input.description || "") ||
        (Array.isArray(input.nodeIds) && input.nodeIds.length) ||
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
    if (shouldCompileCanvasApplyAsLayout(input, snapshot)) {
        return compileCanvasLayoutOps(input, snapshot);
    }
    const ops: CanvasAgentOp[] = [];
    const resolvedDeletes = resolveCanvasAgentNodeIds(snapshot, input.deleteIds || []);
    if ((input.deleteIds || []).length && resolvedDeletes.ids.length === 0 && Array.isArray(input.nodes) && input.nodes.length) {
        throw new Error("deleteIds 已不在画布上，拒绝再创建副本。请 inspect 后用 patches.position 移动现有节点。");
    }
    if (resolvedDeletes.ids.length) ops.push({ type: "delete_node", ids: resolvedDeletes.ids });
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
    const meaningful = dropNoopCanvasUpdateOps(ops, snapshot);
    if (!meaningful.length) throw new Error(APPLY_ALREADY_SATISFIED_MESSAGE);
    return meaningful;
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
    const ir = resolveCreationIr(snapshot.alloCreative, snapshot.nodes);
    const creation = summarizeCreationForAgent(ir);
    const focus = typeof args.focus === "string" ? args.focus : "";
    const domain = CREATION_INSPECT_FOCUS.has(focus)
        ? summarizeCreationDomain(ir, focus, snapshot.alloCreative)
        : undefined;
    const payload = ids.length === 1 && !query
        ? { observation, node: getCanvasAgentNode(snapshot, { id: ids[0] }) }
        : query || ids.length || types?.length
            ? { observation, ...findCanvasAgentNodes(snapshot, { query, ids, types, limit: typeof args.limit === "number" ? args.limit : 30 }) }
            : args.focus === "resources"
                ? { observation, ...getCanvasAgentResources(snapshot, { limit: 50 }) }
                : domain
                    ? { observation }
                    : { observation, graph: summarizeGraph(snapshot, observation) };
    return {
        ...payload,
        ...(creation ? { creation } : {}),
        ...(domain ? { focus, domain } : {}),
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

function canvasNodePrompt(node: CanvasNodeData) {
    return String(node.metadata?.prompt || node.metadata?.composerContent || node.metadata?.content || "").trim();
}

function isBusyGeneratingCanvasNode(node: CanvasNodeData) {
    return ["pending", "loading", "queued", "running", "processing"].includes(String(node.metadata?.status || "idle"));
}

function isRunnableCanvasTarget(node: CanvasNodeData) {
    if (!generationModeForNode(node)) return false;
    if (unchangedModeratedPrompt(node.metadata, canvasNodePrompt(node))) return false;
    if (isBusyGeneratingCanvasNode(node)) return false;
    const status = String(node.metadata?.status || "idle");
    if (status === "success" && (node.metadata?.storageKey || node.metadata?.primaryImageId)) return false;
    return Boolean(canvasNodePrompt(node));
}

export function resolveRunTargets(snapshot: CanvasAgentSnapshot, nodeIds?: string[]) {
    if (nodeIds?.length) {
        const resolved = resolveCanvasAgentNodeIds(snapshot, nodeIds);
        if (resolved.missing.length) throw new Error(`找不到节点：${resolved.missing.join(", ")}`);
        const nodes = resolved.ids.map((id) => snapshot.nodes.find((node) => node.id === id)!).filter(Boolean);
        const runnable = nodes.filter(isRunnableCanvasTarget);
        if (runnable.length) return runnable;
        if (nodes.some((node) => unchangedModeratedPrompt(node.metadata, canvasNodePrompt(node)))) throw new Error(CANVAS_AGENT_MODERATION_RERUN_MESSAGE);
        if (nodes.some(isBusyGeneratingCanvasNode)) throw new Error(CANVAS_AGENT_BUSY_RERUN_MESSAGE);
        throw new Error("没有可生成的节点。请指定 nodeIds，或先 apply 带 prompt 的图片/视频节点。");
    }
    return snapshot.nodes.filter(isRunnableCanvasTarget);
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
    const candidates = snapshot.nodes.filter((node) => {
        const status = String(node.metadata?.status || "idle");
        return Boolean(generationModeForNode(node)) && (status === "error" || status === "failed" || status === "idle");
    });
    const runnable = candidates.filter(isRunnableCanvasTarget);
    if (runnable.length) return runnable;
    if (candidates.some((node) => unchangedModeratedPrompt(node.metadata, canvasNodePrompt(node)))) {
        throw new Error(CANVAS_AGENT_MODERATION_RERUN_MESSAGE);
    }
    return [];
}

function inferRepairAction(input: CanvasRepairInput) {
    if (input.patches?.length) return "patch";
    if (input.nodeIds?.length) return "rerun";
    return "rewire_refs";
}

function patchToOp(snapshot: CanvasAgentSnapshot, patch: CanvasApplyPatch): CanvasAgentOp {
    const id = resolveCanvasAgentNodeId(snapshot, patch.id);
    if (!id) throw new Error(`找不到要修补的节点：${patch.id}`);
    const geometry = extractPatchGeometry(patch);
    const nodePatch: Partial<CanvasNodeData> = {
        ...(patch.title ? { title: patch.title } : {}),
        ...(geometry.position ? { position: geometry.position } : {}),
        ...(geometry.width != null ? { width: geometry.width } : {}),
        ...(geometry.height != null ? { height: geometry.height } : {}),
    };
    return {
        type: "update_node",
        id,
        patch: Object.keys(nodePatch).length ? nodePatch : undefined,
        metadata: {
            ...(patch.content !== undefined ? { content: patch.content } : {}),
            ...(patch.prompt !== undefined ? { prompt: patch.prompt, composerContent: patch.prompt } : {}),
            ...(patch.seconds !== undefined ? { seconds: patch.seconds } : {}),
            ...geometry.metadata,
        },
    };
}

function dropNoopCanvasUpdateOps(ops: CanvasAgentOp[], snapshot: CanvasAgentSnapshot) {
    const nodeById = new Map(snapshot.nodes.map((node) => [node.id, node]));
    const filtered = ops.filter((op) => {
        if (op.type !== "update_node") return true;
        const node = nodeById.get(op.id);
        if (!node) return true;
        if (op.patch?.title && op.patch.title !== node.title) return true;
        if (op.patch?.width != null && op.patch.width !== node.width) return true;
        if (op.patch?.height != null && op.patch.height !== node.height) return true;
        if (op.patch?.position && (Math.abs(op.patch.position.x - node.position.x) > 0.5 || Math.abs(op.patch.position.y - node.position.y) > 0.5)) return true;
        if (op.patch?.parentId && op.patch.parentId !== node.parentId) return true;
        const metadata = op.metadata && typeof op.metadata === "object" ? op.metadata as Record<string, unknown> : {};
        return Object.entries(metadata).some(([key, value]) => value !== undefined && JSON.stringify((node.metadata as Record<string, unknown> | undefined)?.[key]) !== JSON.stringify(value));
    });
    if (filtered.some((op) => op.type !== "select_nodes" && op.type !== "set_viewport")) return filtered;
    return [];
}

function summarizeGraph(snapshot: CanvasAgentSnapshot, observation: CanvasAgentObservation) {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    return {
        nodes: snapshot.nodes.map((node) => ({
            id: node.id,
            shortId: canvasAgentShortId(node.id, aliases),
            type: node.type,
            title: node.title,
            x: Math.round(node.position.x),
            y: Math.round(node.position.y),
            width: node.width,
            height: node.height,
            status: node.metadata?.status || "idle",
            prompt: String(node.metadata?.prompt || node.metadata?.composerContent || "").slice(0, 180),
            appliedTemplateId: node.metadata?.appliedTemplate?.id,
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
