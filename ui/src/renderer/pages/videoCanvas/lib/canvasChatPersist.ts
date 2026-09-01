import type { CanvasAssistantMessage, CanvasAssistantReference, CanvasAssistantSession } from "@oc/types/canvas";
import type { CanvasProject } from "@oc/stores/canvas/use-canvas-store";
import type { CanvasDocument } from "../types";

/** Strip bulky media so Agent sessions can ride along the canvas doc PUT. */
export function persistableChatSessions(sessions: CanvasAssistantSession[] | undefined): CanvasAssistantSession[] {
    return (sessions || [])
        .filter((session) => session.messages.some((message) => message.role === "user" || message.role === "assistant" || message.role === "tool"))
        .map((session) => ({
            id: session.id,
            title: session.title,
            createdAt: session.createdAt,
            updatedAt: session.updatedAt,
            messages: session.messages.map(persistableChatMessage),
        }));
}

export function parsePersistedChatSessions(value: unknown): CanvasAssistantSession[] {
    if (!Array.isArray(value)) return [];
    return value.flatMap((item) => {
        if (!item || typeof item !== "object") return [];
        const record = item as Record<string, unknown>;
        if (typeof record.id !== "string" || !record.id) return [];
        const messages = Array.isArray(record.messages) ? record.messages.flatMap(parsePersistedMessage) : [];
        return [{
            id: record.id,
            title: typeof record.title === "string" && record.title.trim() ? record.title : "新对话",
            messages,
            createdAt: typeof record.createdAt === "string" ? record.createdAt : new Date().toISOString(),
            updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : new Date().toISOString(),
        }];
    });
}

function persistableChatMessage(message: CanvasAssistantMessage): CanvasAssistantMessage {
    return {
        id: message.id,
        role: message.role,
        ...(message.title ? { title: message.title } : {}),
        text: message.text,
        ...(message.meta ? { meta: message.meta } : {}),
        ...(message.modelContext ? { modelContext: message.modelContext } : {}),
        ...(message.references?.length ? { references: message.references.map(persistableReference) } : {}),
        ...(message.detail !== undefined ? { detail: persistableDetail(message.detail) } : {}),
    };
}

function persistableReference(item: CanvasAssistantReference): CanvasAssistantReference {
    return {
        id: item.id,
        type: item.type,
        title: item.title,
        ...(item.storageKey ? { storageKey: item.storageKey } : {}),
        ...(item.text ? { text: item.text } : {}),
    };
}

function persistableDetail(detail: unknown): unknown {
    if (!detail || typeof detail !== "object") return detail;
    if (Array.isArray(detail)) return detail.map(persistableDetail);
    const record = detail as Record<string, unknown>;
    const next: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(record)) {
        if (key === "data" || key === "before" || key === "after" || key === "snapshot" || key === "impact") continue;
        if (key === "result" && value && typeof value === "object" && !Array.isArray(value)) {
            const result = value as Record<string, unknown>;
            next.result = { ok: result.ok, message: result.message };
            continue;
        }
        if (key === "results" && Array.isArray(value)) {
            next.results = value.map((item) => {
                if (!item || typeof item !== "object") return item;
                const row = item as Record<string, unknown>;
                const result = row.result && typeof row.result === "object" ? row.result as Record<string, unknown> : undefined;
                return {
                    toolCallId: row.toolCallId,
                    name: row.name,
                    result: result ? { ok: result.ok, message: result.message } : row.result,
                };
            });
            continue;
        }
        next[key] = value;
    }
    return next;
}

function parsePersistedMessage(value: unknown): CanvasAssistantMessage[] {
    if (!value || typeof value !== "object") return [];
    const record = value as Record<string, unknown>;
    if (typeof record.id !== "string" || typeof record.text !== "string") return [];
    const role = record.role;
    if (role !== "user" && role !== "assistant" && role !== "system" && role !== "tool" && role !== "error") return [];
    return [{
        id: record.id,
        role,
        text: record.text,
        ...(typeof record.title === "string" ? { title: record.title } : {}),
        ...(typeof record.meta === "string" ? { meta: record.meta } : {}),
        ...(typeof record.modelContext === "string" ? { modelContext: record.modelContext } : {}),
        ...(Array.isArray(record.references) ? { references: record.references.filter(isPersistedReference) } : {}),
        ...(record.detail !== undefined ? { detail: record.detail } : {}),
    }];
}

function isPersistedReference(value: unknown): value is CanvasAssistantReference {
    if (!value || typeof value !== "object") return false;
    const record = value as Record<string, unknown>;
    return typeof record.id === "string" && typeof record.title === "string" && typeof record.type === "string";
}

export function projectToCanvasDocument(project: CanvasProject): CanvasDocument {
    const chatSessions = persistableChatSessions(project.chatSessions);
    return {
        schema: 1,
        title: project.title,
        nodes: project.nodes as unknown as CanvasDocument["nodes"],
        connections: project.connections as unknown as CanvasDocument["connections"],
        viewport: project.viewport as CanvasDocument["viewport"],
        backgroundMode: (project.backgroundMode as CanvasDocument["backgroundMode"]) || "dots",
        ...(project.timeline ? { timeline: project.timeline as CanvasDocument["timeline"] } : {}),
        ...(project.alloCreative ? { alloCreative: project.alloCreative } : {}),
        ...(chatSessions.length
            ? {
                chatSessions,
                activeChatId: chatSessions.some((session) => session.id === project.activeChatId)
                    ? project.activeChatId
                    : chatSessions[0]?.id ?? null,
            }
            : {}),
    };
}
