import { resolveCanvasAgentNodeId, resolveCanvasAgentNodeIds } from "./canvas-agent-ids";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "./canvas-agent-ops";
import { nodeTypeForWorkflowKind, type CanvasWorkflowNodeInput } from "./canvas-agent-workflow";
import { CanvasNodeType, type CanvasNodeData, type CanvasNodeMetadata } from "@oc/types/canvas";

const DEFAULT_GAP = 80;
const POSITION_EPSILON = 0.5;
const WORKFLOW_CLONE_PREFIX = "agent-workflow-";

export type CanvasLayoutInput = {
    title?: string;
    description?: string;
    layout?: boolean;
    nodes?: CanvasWorkflowNodeInput[];
    edges?: Array<{ from: string; to: string }>;
    deleteIds?: string[];
    nodeIds?: string[];
    direction?: "horizontal" | "vertical";
    start?: { x: number; y: number };
    gap?: number;
};

export const APPLY_ALREADY_SATISFIED_MESSAGE = "目标状态已满足：现有节点已在目标位置。不要再 canvas_apply，禁止删除后重建。";
export const APPLY_REFUSE_DUPLICATE_MESSAGE = "拒绝再创建副本：要删除的节点已不在画布上，且将写入的节点与现有图重复。请 inspect 后用 patches.position 移动现有节点。";

export function looksLikeCanvasLayoutRequest(value: string) {
    return /整理画布|整理节点|重叠|overlap|对齐|排列|排开|铺开|错开|layout|不重叠|不要重叠|不在重叠|重新排列|纵向布局|横向布局|间距/.test(value);
}

export function looksLikeDuplicateGraphRequest(value: string) {
    return /再[做建创]|另外[一再]?套|再来[一]?套|复制一份|再搭/.test(value);
}

export function isCanvasApplyAlreadySatisfiedError(error: unknown) {
    const message = error instanceof Error ? error.message : String(error || "");
    return message === APPLY_ALREADY_SATISFIED_MESSAGE || message.includes("目标状态已满足");
}

export function isCanvasApplyRefuseDuplicateError(error: unknown) {
    const message = error instanceof Error ? error.message : String(error || "");
    return message === APPLY_REFUSE_DUPLICATE_MESSAGE || message.includes("拒绝再创建副本");
}

export function sameCanvasPosition(left?: { x: number; y: number }, right?: { x: number; y: number }) {
    if (!left || !right) return false;
    return Math.abs(left.x - right.x) < POSITION_EPSILON && Math.abs(left.y - right.y) < POSITION_EPSILON;
}

export function extractPatchGeometry(patch: {
    position?: unknown;
    x?: unknown;
    y?: unknown;
    width?: unknown;
    height?: unknown;
    metadata?: Record<string, unknown>;
}) {
    const metadata = patch.metadata && typeof patch.metadata === "object" && !Array.isArray(patch.metadata) ? patch.metadata : {};
    const nestedPosition = asPosition(metadata.position) || nestedXY(metadata);
    const position = asPosition(patch.position) || nestedXY(patch) || nestedPosition;
    const width = asFiniteNumber(patch.width) ?? asFiniteNumber(metadata.width);
    const height = asFiniteNumber(patch.height) ?? asFiniteNumber(metadata.height);
    const { position: _position, x: _x, y: _y, width: _width, height: _height, ...restMetadata } = metadata;
    return {
        position,
        width,
        height,
        metadata: restMetadata,
    };
}

export function omitGeometryMetadata(metadata: Record<string, unknown> | undefined) {
    if (!metadata) return {};
    const { position: _position, x: _x, y: _y, width: _width, height: _height, ...rest } = metadata;
    return rest;
}

export function matchExistingNodesForApply(
    snapshot: CanvasAgentSnapshot,
    proposed: CanvasWorkflowNodeInput[],
    deleteIds: string[] = [],
    hint = "",
) {
    if (!proposed.length) return null;
    const resolvedDeletes = resolveCanvasAgentNodeIds(snapshot, deleteIds).ids
        .map((id) => snapshot.nodes.find((node) => node.id === id))
        .filter((node): node is CanvasNodeData => Boolean(node));
    if (resolvedDeletes.length === proposed.length && resolvedDeletes.every((node, index) => node.type === nodeTypeForWorkflowKind(proposed[index].kind))) {
        const titleHits = proposed.filter((item, index) => resolvedDeletes[index]?.title.trim() === item.title.trim()).length;
        if (looksLikeCanvasLayoutRequest(hint) || titleHits >= Math.ceil(proposed.length / 2)) return resolvedDeletes;
    }
    const used = new Set<string>();
    const matched: CanvasNodeData[] = [];
    for (const item of proposed) {
        const type = nodeTypeForWorkflowKind(item.kind);
        const title = item.title.trim();
        const prompt = String(item.prompt || item.content || "").trim();
        const candidates = snapshot.nodes.filter((node) => !used.has(node.id) && node.type === type);
        const byTitle = title ? candidates.filter((node) => node.title.trim() === title) : [];
        const byPrompt = prompt
            ? candidates.filter((node) => String(node.metadata?.prompt || node.metadata?.composerContent || "").includes(prompt.slice(0, 24)))
            : [];
        const pick = preferOriginalNode(byTitle) || preferOriginalNode(byPrompt);
        if (!pick) return null;
        used.add(pick.id);
        matched.push(pick);
    }
    return matched.length === proposed.length ? matched : null;
}

export function shouldCompileCanvasApplyAsLayout(input: CanvasLayoutInput, snapshot: CanvasAgentSnapshot) {
    if (!snapshot.nodes.length) return false;
    if (looksLikeDuplicateGraphRequest(`${input.title || ""} ${input.description || ""}`)) return false;
    const hint = `${input.title || ""} ${input.description || ""}`;
    const proposed = input.nodes || [];
    const matched = proposed.length ? matchExistingNodesForApply(snapshot, proposed, input.deleteIds || [], hint) : null;
    if (input.layout === true) return true;
    const layoutRequest = looksLikeCanvasLayoutRequest(hint);
    if (layoutRequest && !proposed.length) return true;
    if (layoutRequest && matched) return true;
    if (matched && ((input.deleteIds || []).length || input.direction || input.gap != null)) return true;
    const resolvedDeletes = resolveCanvasAgentNodeIds(snapshot, input.deleteIds || []);
    if (proposed.length && matched && (input.deleteIds || []).length && resolvedDeletes.ids.length === 0) return true;
    return false;
}

export function compileCanvasLayoutOps(input: CanvasLayoutInput, snapshot: CanvasAgentSnapshot): CanvasAgentOp[] {
    const hint = `${input.title || ""} ${input.description || ""}`;
    const proposed = input.nodes || [];
    const matched = proposed.length
        ? matchExistingNodesForApply(snapshot, proposed, input.deleteIds || [], hint)
        : resolveLayoutTargets(snapshot, input.nodeIds);
    if (!matched?.length) {
        const resolvedDeletes = resolveCanvasAgentNodeIds(snapshot, input.deleteIds || []);
        if (proposed.length && (input.deleteIds || []).length && resolvedDeletes.ids.length === 0) {
            throw new Error(APPLY_REFUSE_DUPLICATE_MESSAGE);
        }
        throw new Error("没有可排列的现有节点。请 inspect 后用 patches.position 移动，或给出 nodeIds。");
    }
    const keepIds = new Set(matched.map((node) => node.id));
    const titles = new Set(matched.map((node) => node.title.trim()).filter(Boolean));
    const extras = snapshot.nodes
        .filter((node) => !keepIds.has(node.id) && node.id.startsWith(WORKFLOW_CLONE_PREFIX) && titles.has(node.title.trim()))
        .map((node) => node.id);
    const direction = input.direction === "horizontal" ? "horizontal" : "vertical";
    const gap = Math.max(48, input.gap ?? DEFAULT_GAP);
    const packed = packCanvasNodes(matched, direction, gap, input.start);
    const refToId = new Map(proposed.map((item, index) => [item.ref, matched[index]!.id]));
    const ops: CanvasAgentOp[] = [];
    if (extras.length) ops.push({ type: "delete_node", ids: extras });
    packed.forEach((position, index) => {
        const node = matched[index]!;
        const proposedNode = proposed[index];
        const patch: Partial<CanvasNodeData> = {};
        if (!sameCanvasPosition(node.position, position)) patch.position = position;
        if (proposedNode?.title && proposedNode.title.trim() !== node.title.trim()) patch.title = proposedNode.title;
        const metadata: Record<string, unknown> = {};
        if (proposedNode?.content !== undefined && proposedNode.content !== node.metadata?.content) metadata.content = proposedNode.content;
        if (proposedNode?.prompt !== undefined) {
            const currentPrompt = String(node.metadata?.prompt || node.metadata?.composerContent || "");
            if (proposedNode.prompt !== currentPrompt) {
                metadata.prompt = proposedNode.prompt;
                metadata.composerContent = proposedNode.prompt;
            }
        }
        if (!Object.keys(patch).length && !Object.keys(metadata).length) return;
        ops.push({
            type: "update_node",
            id: node.id,
            patch: Object.keys(patch).length ? patch : undefined,
            metadata: Object.keys(metadata).length ? metadata as CanvasNodeMetadata : undefined,
        });
    });
    const edges = input.edges?.length
        ? input.edges
        : proposed.length > 1
            ? proposed.slice(0, -1).map((node, index) => ({ from: node.ref, to: proposed[index + 1]!.ref }))
            : [];
    for (const edge of edges) {
        const fromNodeId = refToId.get(edge.from) || resolveCanvasAgentNodeId(snapshot, edge.from);
        const toNodeId = refToId.get(edge.to) || resolveCanvasAgentNodeId(snapshot, edge.to);
        if (!fromNodeId || !toNodeId || fromNodeId === toNodeId) continue;
        const exists = snapshot.connections.some((connection) => connection.fromNodeId === fromNodeId && connection.toNodeId === toNodeId);
        if (!exists) ops.push({ type: "connect_nodes", fromNodeId, toNodeId });
    }
    if (!ops.length) throw new Error(APPLY_ALREADY_SATISFIED_MESSAGE);
    ops.push({ type: "select_nodes", ids: matched.map((node) => node.id) });
    return ops;
}

export function packCanvasNodes(
    nodes: CanvasNodeData[],
    direction: "horizontal" | "vertical",
    gap: number,
    start?: { x: number; y: number },
) {
    const origin = start || {
        x: nodes.reduce((min, node) => Math.min(min, node.position.x), nodes[0]?.position.x ?? 80),
        y: nodes.reduce((min, node) => Math.min(min, node.position.y), nodes[0]?.position.y ?? 80),
    };
    let cursor = { ...origin };
    return nodes.map((node) => {
        const position = { ...cursor };
        if (direction === "vertical") cursor = { x: origin.x, y: cursor.y + node.height + gap };
        else cursor = { x: cursor.x + node.width + gap, y: origin.y };
        return position;
    });
}

function resolveLayoutTargets(snapshot: CanvasAgentSnapshot, nodeIds?: string[]) {
    if (nodeIds?.length) {
        const resolved = resolveCanvasAgentNodeIds(snapshot, nodeIds);
        return resolved.ids.map((id) => snapshot.nodes.find((node) => node.id === id)).filter((node): node is CanvasNodeData => Boolean(node));
    }
    if (snapshot.selectedNodeIds.length) {
        const selected = snapshot.nodes.filter((node) => snapshot.selectedNodeIds.includes(node.id) && node.type !== CanvasNodeType.Frame);
        if (selected.length) return selected;
    }
    return snapshot.nodes.filter((node) => node.type !== CanvasNodeType.Frame);
}

function preferOriginalNode(nodes: CanvasNodeData[]) {
    if (!nodes.length) return undefined;
    return nodes.find((node) => !node.id.startsWith(WORKFLOW_CLONE_PREFIX)) || nodes[0];
}

function asPosition(value: unknown): { x: number; y: number } | undefined {
    if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
    const record = value as Record<string, unknown>;
    const x = asFiniteNumber(record.x);
    const y = asFiniteNumber(record.y);
    if (x == null || y == null) return undefined;
    return { x, y };
}

function nestedXY(value: { x?: unknown; y?: unknown }) {
    const x = asFiniteNumber(value.x);
    const y = asFiniteNumber(value.y);
    if (x == null || y == null) return undefined;
    return { x, y };
}

function asFiniteNumber(value: unknown) {
    return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
