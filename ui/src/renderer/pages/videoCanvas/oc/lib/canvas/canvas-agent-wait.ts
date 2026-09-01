import { getCanvasAgentGenerationTasks } from "./canvas-agent-context";
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
        if (!pendingCount && (waitedForSubmit || !input.nodeIds?.length)) {
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
        return status === "pending" || status === "running" || status === "processing";
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
