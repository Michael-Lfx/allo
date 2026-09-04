import { getCanvasAgentGenerationTasks } from "./canvas-agent-context";
import { CanvasNodeType } from "@oc/types/canvas";
import type { CanvasAgentSnapshot } from "./canvas-agent-ops";

const DEFAULT_TIMEOUT_MS = 90_000;
const DEFAULT_POLL_MS = 1_500;
const TERMINAL = new Set(["succeeded", "failed", "cancelled", "not_started"]);

export type CanvasAgentWaitGenerationInput = {
    nodeIds?: string[];
    timeoutMs?: number;
};

export type CanvasAgentWaitGenerationResult = {
    timedOut: boolean;
    elapsedMs: number;
    pendingCount: number;
    tasks: ReturnType<typeof getCanvasAgentGenerationTasks>["tasks"];
};

export async function waitCanvasAgentGeneration(
    readSnapshot: () => CanvasAgentSnapshot,
    input: CanvasAgentWaitGenerationInput = {},
    options: { now?: () => number; sleep?: (ms: number) => Promise<void> } = {},
): Promise<CanvasAgentWaitGenerationResult> {
    const timeoutMs = clamp(input.timeoutMs ?? DEFAULT_TIMEOUT_MS, 3_000, 180_000);
    const now = options.now || Date.now;
    const sleep = options.sleep || delay;
    const started = now();
    const deadline = started + timeoutMs;

    let polls = 0;
    while (true) {
        const snapshot = readSnapshot();
        const { tasks } = getCanvasAgentGenerationTasks(snapshot, { nodeIds: input.nodeIds, limit: 200 });
        const generatingNodeCount = countGeneratingNodes(snapshot, input.nodeIds);
        const pendingTasks = tasks.filter((task) => !isTerminalTask(task.status, task.officialStatus, task.nodeStatus));
        const pendingCount = Math.max(pendingTasks.length, generatingNodeCount);
        const waitedForSubmit = polls >= 2;
        const alreadyDone = Boolean(input.nodeIds?.length) && !generatingNodeCount && !pendingTasks.length && snapshot.nodes
            .filter((node) => input.nodeIds!.includes(node.id))
            .every((node) => {
                const status = String(node.metadata?.status || "");
                return status === "success" || status === "error" || status === "cancelled" || Boolean(node.metadata?.content);
            });
        if (!pendingCount && (waitedForSubmit || !input.nodeIds?.length || alreadyDone)) {
            return { timedOut: false, elapsedMs: now() - started, pendingCount: 0, tasks };
        }
        if (now() >= deadline) {
            return { timedOut: true, elapsedMs: now() - started, pendingCount, tasks };
        }
        polls += 1;
        await sleep(Math.min(DEFAULT_POLL_MS, Math.max(200, deadline - now())));
    }
}

function countGeneratingNodes(snapshot: CanvasAgentSnapshot, nodeIds?: string[]) {
    const filter = nodeIds?.length ? new Set(nodeIds) : null;
    return snapshot.nodes.filter((node) => {
        if (filter && !filter.has(node.id)) return false;
        const status = String(node.metadata?.status || "");
        return status === "pending" || status === "running" || status === "processing" || status === "loading" || status === "queued";
    }).length;
}

function isTerminalTask(status: string, officialStatus?: string, nodeStatus?: string) {
    const outcome = taskOutcome(status, officialStatus, nodeStatus);
    return TERMINAL.has(outcome);
}

function taskOutcome(status: string, officialStatus?: string, nodeStatus?: string) {
    const value = status || officialStatus || nodeStatus || "unknown";
    if (value === "queued" || value === "pending") return "queued";
    if (value === "running" || value === "processing") return "running";
    if (value === "succeeded" || value === "completed" || value === "success") return "succeeded";
    if (value === "failed" || value === "error") return "failed";
    if (value === "cancelled" || value === "canceled") return "cancelled";
    if (value === "idle" && !status) return "not_started";
    return value;
}

function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
}

function delay(ms: number) {
    return new Promise<void>((resolve) => {
        window.setTimeout(resolve, ms);
    });
}

const INBOUND_BUSY = new Set(["pending", "running", "processing", "loading", "queued"]);

export async function waitForInboundCanvasImages(
    readSnapshot: () => CanvasAgentSnapshot,
    nodeId: string,
    options: { timeoutMs?: number; now?: () => number; sleep?: (ms: number) => Promise<void> } = {},
) {
    const timeoutMs = clamp(options.timeoutMs ?? DEFAULT_TIMEOUT_MS, 1_000, 180_000);
    const now = options.now || Date.now;
    const sleep = options.sleep || delay;
    const started = now();
    const deadline = started + timeoutMs;
    while (true) {
        const snapshot = readSnapshot();
        const inboundIds = new Set(snapshot.connections.filter((connection) => connection.toNodeId === nodeId).map((connection) => connection.fromNodeId));
        const inboundImages = snapshot.nodes.filter((node) => inboundIds.has(node.id) && node.type === CanvasNodeType.Image);
        if (!inboundImages.length) return { timedOut: false, imageIds: [] as string[], ready: true };
        const readyIds = inboundImages.filter((node) => Boolean(node.metadata?.content)).map((node) => node.id);
        const awaiting = inboundImages.filter((node) => {
            if (node.metadata?.content) return false;
            const status = String(node.metadata?.status || "idle");
            if (status === "error" || status === "cancelled") return false;
            if (INBOUND_BUSY.has(status)) return true;
            return Boolean(node.metadata?.prompt || node.metadata?.composerContent);
        });
        if (!awaiting.length) return { timedOut: false, imageIds: readyIds, ready: readyIds.length === inboundImages.length };
        if (now() >= deadline) return { timedOut: true, imageIds: readyIds, ready: readyIds.length === inboundImages.length };
        await sleep(Math.min(DEFAULT_POLL_MS, Math.max(200, deadline - now())));
    }
}
