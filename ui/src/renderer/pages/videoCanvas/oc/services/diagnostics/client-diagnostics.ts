/**
 * Allo 没有 OC 的统一诊断上报通道。导演台仍走同一套 recorder API，
 * 这里只做进程内缓冲，避免把诊断耦合到缺失的后端。
 */

export type DiagnosticLevel = "debug" | "info" | "warn" | "error";

export type ClientDiagnosticEvent = {
    level: DiagnosticLevel;
    category: string;
    code: string;
    message: string;
};

const MAX_EVENTS = 80;
const events: ClientDiagnosticEvent[] = [];

export function recordDiagnosticEvent(event: ClientDiagnosticEvent) {
    events.unshift(event);
    if (events.length > MAX_EVENTS) events.length = MAX_EVENTS;
}

export function readDiagnosticEvents() {
    return [...events];
}
