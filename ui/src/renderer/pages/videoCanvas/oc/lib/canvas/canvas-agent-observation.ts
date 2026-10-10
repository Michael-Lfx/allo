import { buildCanvasAgentAliasMap, canvasAgentNodeChangeKind, canvasAgentShortId } from "./canvas-agent-ids";
import { findCanvasNodeOverlaps, hashCanvasAgentSnapshot, type CanvasAgentPostcondition, type CanvasAgentSnapshot } from "./canvas-agent-ops";
import { canvasAgentGoalStillOpen, canvasAgentIdleMedia, canvasAgentMissingFilmNodes, canvasAgentModerationBlocked, type CanvasAgentGoalHint } from "./canvas-agent-policy";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

export const CANVAS_AGENT_CODES = {
    OK: "OK",
    VALIDATE_FAILED: "VALIDATE_FAILED",
    HASH_STALE: "HASH_STALE",
    MISSING_REF: "MISSING_REF",
    UPSTREAM_EMPTY: "UPSTREAM_EMPTY",
    SUBMIT_PENDING: "SUBMIT_PENDING",
    WAIT_TIMEOUT: "WAIT_TIMEOUT",
    GOAL_INCOMPLETE: "GOAL_INCOMPLETE",
    GENERATION_FAILED: "GENERATION_FAILED",
    OVERLAP_WARN: "OVERLAP_WARN",
    NOOP: "NOOP",
    APPLY_NEEDS_NODES: "APPLY_NEEDS_NODES",
} as const;

export type CanvasAgentCode = (typeof CANVAS_AGENT_CODES)[keyof typeof CANVAS_AGENT_CODES];

export type CanvasAgentQueueItem = {
    id: string;
    shortId: string;
    title: string;
    type: CanvasNodeData["type"];
    status: string;
    ready: boolean;
    taskId?: string;
};

export type CanvasAgentObservation = {
    fingerprint: string;
    revision: number;
    nodeCount: number;
    connectionCount: number;
    selected: string[];
    templates: string[];
    queue: CanvasAgentQueueItem[];
    ready: CanvasAgentQueueItem[];
    failed: CanvasAgentQueueItem[];
    idle: CanvasAgentQueueItem[];
    diff: { new: string[]; modified: string[] };
    warnings: string[];
    incomplete: boolean;
};

const BUSY = new Set(["pending", "loading", "queued", "running", "processing"]);
const FAILED = new Set(["error", "failed", "cancelled", "canceled"]);

export function buildCanvasAgentObservation(snapshot: CanvasAgentSnapshot, previous?: CanvasAgentSnapshot | null, goal?: CanvasAgentGoalHint): CanvasAgentObservation {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    const previousById = new Map((previous?.nodes || []).map((node) => [node.id, node]));
    const items = snapshot.nodes.map((node) => toQueueItem(node, aliases));
    const queue = items.filter((item) => BUSY.has(item.status));
    const failed = items.filter((item) => FAILED.has(item.status));
    const ready = items.filter((item) => item.ready);
    const idleNodes = canvasAgentIdleMedia(snapshot);
    const idleIds = new Set(idleNodes.map((node) => node.id));
    const idle = items.filter((item) => idleIds.has(item.id));
    const diff = {
        new: snapshot.nodes.filter((node) => canvasAgentNodeChangeKind(previousById.get(node.id), node) === "new").map((node) => `${canvasAgentShortId(node.id, aliases)}:${node.title || node.type}`),
        modified: snapshot.nodes.filter((node) => canvasAgentNodeChangeKind(previousById.get(node.id), node) === "modified").map((node) => `${canvasAgentShortId(node.id, aliases)}:${node.title || node.type}`),
    };
    const overlapWarnings = findCanvasNodeOverlaps(snapshot.nodes);
    const goalOpen = goal ? canvasAgentGoalStillOpen(snapshot, goal) : false;
    const warnings = [
        ...(failed.length ? [`${failed.length} 个节点生成失败，先 canvas_critique 再 canvas_repair，不要整图重来。`] : []),
        ...(queue.length ? [`${queue.length} 个生成任务仍在队列中，不要把未就绪资源当成完成。`] : []),
        ...(idle.length && goalOpen ? [`${idle.length} 个媒体节点仍 idle。请 canvas_run，不要宣称完成。`] : []),
        ...(goal && canvasAgentModerationBlocked(snapshot) ? ["内容审核未通过且提示词未改。禁止 canvas_run / repair rerun，不要再请求确认。"] : []),
        ...(goal?.production && canvasAgentMissingFilmNodes(snapshot) && !canvasAgentModerationBlocked(snapshot) ? ["分镜已在但还没有镜头视频/成片节点。继续补镜头，不要停止。"] : []),
        ...(overlapWarnings.length ? [`${overlapWarnings.length} 组节点重叠。用 patches.position 移动现有节点，禁止 deleteIds 后重建副本。`] : []),
        ...((snapshot.selectedNodeIds || []).length ? ["当前选区已在观察中，优先复用选中节点。"] : []),
    ];
    return {
        fingerprint: canvasGraphFingerprint(snapshot),
        revision: snapshot.revision ?? 0,
        nodeCount: snapshot.nodes.length,
        connectionCount: snapshot.connections.length,
        selected: (snapshot.selectedNodeIds || []).map((id) => canvasAgentShortId(id, aliases)),
        templates: snapshot.nodes.flatMap((node) => {
            const id = node.metadata?.appliedTemplate?.id;
            return id ? [`${canvasAgentShortId(node.id, aliases)}:${id}`] : [];
        }),
        queue,
        ready,
        failed,
        idle,
        diff,
        warnings,
        incomplete: goalOpen && queue.length === 0,
    };
}

export function canvasGraphFingerprint(snapshot: CanvasAgentSnapshot) {
    return hashCanvasAgentSnapshot({
        ...snapshot,
        selectedNodeIds: [],
        viewport: { x: 0, y: 0, k: 1 },
        nodes: snapshot.nodes.map((node) => ({
            ...node,
            metadata: {
                prompt: String(node.metadata?.prompt || node.metadata?.composerContent || "").slice(0, 200),
                appliedTemplateId: node.metadata?.appliedTemplate?.id,
                content: String(node.metadata?.content || "").slice(0, 80),
                videoStartFrameNodeId: node.metadata?.videoStartFrameNodeId,
                videoEndFrameNodeId: node.metadata?.videoEndFrameNodeId,
                agentAlias: node.metadata?.agentAlias,
            },
        })),
    });
}

export function observationPromptBlock(observation: CanvasAgentObservation) {
    return [
        "[画布观察]",
        `fingerprint=${observation.fingerprint} nodes=${observation.nodeCount} edges=${observation.connectionCount}`,
        observation.selected.length ? `选区：${observation.selected.join(", ")}` : "选区：无",
        observation.templates.length ? `模板：${observation.templates.join("；")}` : "模板：无",
        observation.diff.new.length ? `NEW：${observation.diff.new.join("；")}` : "NEW：无",
        observation.diff.modified.length ? `MODIFIED：${observation.diff.modified.join("；")}` : "MODIFIED：无",
        observation.queue.length
            ? `队列：${observation.queue.map((item) => `${item.shortId} ${item.status}`).join("；")}`
            : "队列：空",
        observation.ready.length
            ? `就绪：${observation.ready.map((item) => item.shortId).join(", ")}`
            : "就绪资源：无",
        observation.failed.length
            ? `失败：${observation.failed.map((item) => `${item.shortId} ${item.status}`).join("；")}`
            : "失败：无",
        observation.idle.length
            ? `idle：${observation.idle.map((item) => item.shortId).join(", ")}`
            : "idle：无",
        ...observation.warnings,
        observation.incomplete ? `code=${CANVAS_AGENT_CODES.GOAL_INCOMPLETE} 任务未完成，不要对用户宣称已完成。` : "",
    ].filter(Boolean).join("\n");
}

export function compactWriteToolData(verification: CanvasAgentPostcondition, snapshot: CanvasAgentSnapshot, previous?: CanvasAgentSnapshot | null, goal?: CanvasAgentGoalHint) {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    const code = writeResultCode(verification);
    return {
        ok: verification.ok,
        code,
        created: verification.createdNodeIds.map((id) => {
            const node = snapshot.nodes.find((item) => item.id === id);
            return { id, shortId: canvasAgentShortId(id, aliases), type: node?.type, title: node?.title };
        }),
        connectionsAdded: verification.createdConnectionIds.length,
        connectionCount: verification.connectionCount,
        generation: verification.generation.map((item) => ({
            nodeId: item.nodeId,
            shortId: canvasAgentShortId(item.nodeId, aliases),
            outcome: item.outcome,
            resourceReady: item.resourceReady,
            message: item.message,
        })),
        warnings: verification.warnings.slice(0, 6),
        observation: buildCanvasAgentObservation(snapshot, previous, goal),
    };
}

export function writeResultCode(verification: CanvasAgentPostcondition): CanvasAgentCode {
    if (verification.generation.some((item) => item.outcome === "failed" || item.outcome === "cancelled")) return CANVAS_AGENT_CODES.GENERATION_FAILED;
    if (verification.generation.some((item) => item.outcome === "not_started")) return CANVAS_AGENT_CODES.SUBMIT_PENDING;
    if (!verification.ok && verification.missingNodeIds.length) return CANVAS_AGENT_CODES.MISSING_REF;
    if (!verification.changed) return CANVAS_AGENT_CODES.NOOP;
    if (verification.overlapWarnings.length) return CANVAS_AGENT_CODES.OVERLAP_WARN;
    if (verification.generation.some((item) => item.outcome === "queued" || item.outcome === "running")) return CANVAS_AGENT_CODES.SUBMIT_PENDING;
    return CANVAS_AGENT_CODES.OK;
}

export function generationModeForNode(node: CanvasNodeData): "text" | "image" | "video" | "audio" | undefined {
    if (node.type === CanvasNodeType.Image) return "image";
    if (node.type === CanvasNodeType.Video) return "video";
    if (node.type === CanvasNodeType.Audio) return "audio";
    if (node.type === CanvasNodeType.Text && node.metadata?.generationMode === "text") return "text";
    if (node.type === CanvasNodeType.Script) return "text";
    return undefined;
}

export function isBusyCanvasStatus(status: string) {
    return BUSY.has(status);
}

function toQueueItem(node: CanvasNodeData, aliases: ReturnType<typeof buildCanvasAgentAliasMap>): CanvasAgentQueueItem {
    const status = String(node.metadata?.status || "idle");
    const storageKey = typeof node.metadata?.storageKey === "string" ? node.metadata.storageKey : "";
    const ready = status === "success" && Boolean(storageKey || node.metadata?.primaryImageId || node.metadata?.resourceId || (typeof node.metadata?.content === "string" && node.metadata.content && node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video));
    return {
        id: node.id,
        shortId: canvasAgentShortId(node.id, aliases),
        title: node.title || "未命名节点",
        type: node.type,
        status,
        ready,
        taskId: typeof node.metadata?.taskId === "string" ? node.metadata.taskId : undefined,
    };
}
