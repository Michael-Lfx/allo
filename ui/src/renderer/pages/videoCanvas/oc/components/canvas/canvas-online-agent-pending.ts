import type { CanvasAssistantMessage, CanvasAssistantSession } from "@oc/types/canvas";
import type { CanvasAgentInputMessage as ResponseInputMessage, CanvasAgentToolCall as ResponseToolCall } from "@oc/lib/canvas/canvas-agent-llm";
import { normalizeResponseToolCall, objectDetail, toolCallsFromDetail } from "./canvas-online-agent-tools";

export type PendingOnlineToolContext = {
    messages: ResponseInputMessage[];
    toolCalls: ResponseToolCall[];
    assistantId: string;
    step: number;
};

const pendingOnlineToolContexts = new Map<string, PendingOnlineToolContext>();

export function stashPendingOnlineToolContext(messageId: string, context: PendingOnlineToolContext) {
    pendingOnlineToolContexts.set(messageId, context);
}

export function dropPendingOnlineToolContext(messageId: string) {
    pendingOnlineToolContexts.delete(messageId);
}

export function resolvePendingOnlineToolContext(
    messageId: string,
    detail: Record<string, unknown>,
    session: CanvasAssistantSession | undefined,
): PendingOnlineToolContext | null {
    const stashed = pendingOnlineToolContexts.get(messageId);
    const toolCalls = stashed?.toolCalls.length ? stashed.toolCalls : toolCallsFromDetail(detail);
    const assistantId = stashed?.assistantId || stringField(detail.assistantId) || lastAssistantId(session);
    const step = stashed?.step || numberField(detail.step) || 1;
    if (!toolCalls.length || !assistantId) return null;
    return {
        messages: stashed?.messages.length ? stashed.messages : [],
        toolCalls,
        assistantId,
        step,
    };
}

export function pendingToolMessageHistory(session: CanvasAssistantSession | undefined): CanvasAssistantMessage[] {
    if (!session) return [];
    return session.messages.filter((message) => message.role === "user" || message.role === "assistant" || message.role === "system");
}

export function lastUserMessage(session: CanvasAssistantSession | undefined): CanvasAssistantMessage | undefined {
    return [...(session?.messages || [])].reverse().find((message) => message.role === "user");
}

function lastAssistantId(session: CanvasAssistantSession | undefined) {
    return [...(session?.messages || [])].reverse().find((message) => message.role === "assistant")?.id || "";
}

function stringField(value: unknown) {
    return typeof value === "string" && value ? value : "";
}

function numberField(value: unknown) {
    return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

export function pendingToolDetail(detail: Record<string, unknown>, context: Pick<PendingOnlineToolContext, "assistantId" | "step" | "toolCalls">) {
    return {
        ...objectDetail(detail),
        assistantId: context.assistantId,
        step: context.step,
        toolCalls: context.toolCalls.flatMap((call) => {
            const next = normalizeResponseToolCall(call);
            return next ? [next] : [];
        }),
    };
}
