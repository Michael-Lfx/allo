import { useRef, useState, type MutableRefObject } from "react";
import { nanoid } from "nanoid";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { compactCanvasAgentSnapshot as compactSnapshot } from "@oc/lib/canvas/canvas-agent-snapshot-compact";
import { isAgentSessionPollingAbort } from "@oc/lib/canvas/canvas-agent-session";
import { summarizeCanvasAgentOps, type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { canvasAgentPromptCacheKey } from "@oc/lib/openai-prompt-cache";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { requestToolResponse, type ResponseInputMessage, type ResponseToolCall } from "@oc/services/api/image";
import { imageToDataUrl } from "@oc/services/image-storage";
import { useConfigStore, type AiConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType, type CanvasAssistantMessage, type CanvasAssistantReference, type CanvasAssistantSession } from "@oc/types/canvas";
import {
    ONLINE_AGENT_MAX_STEPS,
    ONLINE_AGENT_PROMPT,
    ONLINE_AGENT_TOOLS,
    describeCanvasSnapshot,
    isWritableToolCall,
    objectDetail,
    onlineToolToOps,
    parseToolArguments,
    previewOnlineToolCalls,
    summarizeToolCalls,
    toolCallToResponseInput,
    toolCallsFromDetail,
    type OnlineToolResult,
} from "./canvas-online-agent-tools";

export type OnlineAgentLog = { id: string; time: string; title: string; data?: unknown };
type OnlineLoopContext = { step: number };
type OnlineExecutedToolCall = { toolCallId: string; name: string; result: OnlineToolResult };
type PendingOnlineToolContext = { messages: ResponseInputMessage[]; toolCalls: ResponseToolCall[]; assistantId: string; step: number };

export type OnlineAgentLoopOptions = {
    snapshotRef: MutableRefObject<CanvasAgentSnapshot>;
    config: AiConfig;
    confirmTools: boolean;
    selectedReferences: CanvasAssistantReference[];
    activeSession: CanvasAssistantSession | null;
    onApplyOps: (ops?: CanvasAgentOp[]) => CanvasAgentSnapshot;
    appendMessage: (sessionId: string, message: CanvasAssistantMessage) => void;
    upsertMessage: (sessionId: string, message: CanvasAssistantMessage) => void;
    getMessageById: (messageId: string) => CanvasAssistantMessage | undefined;
    getSessionByMessageId: (messageId: string) => CanvasAssistantSession | undefined;
    activateSession: (session: CanvasAssistantSession) => void;
};

export function createSession(): CanvasAssistantSession {
    const now = new Date().toISOString();
    return { id: nanoid(), title: canvasT("videoCanvas.agent.newChatFallback", "新对话"), messages: [], createdAt: now, updatedAt: now };
}

export function useCanvasOnlineAgentLoop({
    snapshotRef,
    config,
    confirmTools,
    selectedReferences,
    activeSession,
    onApplyOps,
    appendMessage,
    upsertMessage,
    getMessageById,
    getSessionByMessageId,
    activateSession,
}: OnlineAgentLoopOptions) {
    const isAiConfigReady = useConfigStore((state) => state.isAiConfigReady);
    const [isRunning, setIsRunning] = useState(false);
    const [onlineLogs, setOnlineLogs] = useState<OnlineAgentLog[]>([]);
    const pendingToolContextRef = useRef(new Map<string, PendingOnlineToolContext>());

    const addOnlineLog = (title: string, data?: unknown) => setOnlineLogs((prev) => [{ id: nanoid(), time: new Date().toLocaleTimeString(), title, data }, ...prev].slice(0, 80));

    const sendMessage = async (text: string, history: CanvasAssistantMessage[], savedReferences?: CanvasAssistantReference[]) => {
        const requestConfig = { ...config, model: config.textModel || config.model };
        if (!isAiConfigReady(requestConfig, requestConfig.model)) {
            navigateToSettings({ continueCreation: true });
            return;
        }

        const session = activeSession || createSession();
        if (!activeSession) activateSession(session);

        const refs = savedReferences || selectedReferences;
        const userMessage: CanvasAssistantMessage = { id: nanoid(), role: "user", text, references: refs };
        const assistantId = nanoid();
        appendMessage(session.id, userMessage);
        addOnlineLog(canvasT("videoCanvas.agent.logSendRequest", "发送请求"), { text, selectedNodeIds: snapshotRef.current.selectedNodeIds, nodeCount: snapshotRef.current.nodes.length, connectionCount: snapshotRef.current.connections.length });
        setIsRunning(true);
        void runOnlineAgentStep(session.id, assistantId, history, userMessage, { step: 1 });
    };

    const runOnlineAgentStep = async (sessionId: string, assistantId: string, history: CanvasAssistantMessage[], userMessage: CanvasAssistantMessage, loop: OnlineLoopContext) => {
        const requestConfig = { ...config, model: config.textModel || config.model };
        try {
            setIsRunning(true);
            const messages = await buildToolAgentMessages(snapshotRef.current, history, userMessage);
            addOnlineLog(canvasT("videoCanvas.agent.logLoopStart", "Agent Tool Loop {{step}} 开始", { step: loop.step }), { toolChoice: "required" });
            let streamed = "";
            const result = await requestToolResponse({ ...requestConfig, systemPrompt: "" }, messages, ONLINE_AGENT_TOOLS, "required", (text) => {
                streamed = text;
                if (text.trim()) upsertMessage(sessionId, { id: assistantId, role: "assistant", text });
            }, { promptCacheKey: canvasAgentPromptCacheKey(sessionId) });
            addOnlineLog(canvasT("videoCanvas.agent.logModelToolReply", "模型工具回复"), result);
            if (result.toolCalls.length) {
                const writableCalls = result.toolCalls.filter(isWritableToolCall);
                if (confirmTools && writableCalls.length) {
                    upsertMessage(sessionId, { id: assistantId, role: "assistant", text: result.content || streamed || canvasT("videoCanvas.agent.preparingWaitConfirm", "准备执行工具，等待确认。") });
                    const toolMessageId = nanoid();
                    pendingToolContextRef.current.set(toolMessageId, { messages, toolCalls: result.toolCalls, assistantId, step: loop.step });
                    const toolMessage: CanvasAssistantMessage = { id: toolMessageId, role: "tool", title: canvasT("videoCanvas.agent.toolConfirmTitle", "确认工具调用"), text: summarizeToolCalls(result.toolCalls), detail: { status: "pending", step: loop.step, toolCalls: result.toolCalls, impact: previewOnlineToolCalls(result.toolCalls, snapshotRef.current, config) } };
                    appendMessage(sessionId, toolMessage);
                    addOnlineLog(canvasT("videoCanvas.agent.logAwaitConfirm", "等待用户确认"), result.toolCalls);
                    return;
                }
                await continueOnlineToolLoop(sessionId, assistantId, messages, result, loop.step);
            } else {
                if (!result.content.trim()) throw new Error(canvasT("videoCanvas.agent.errNoToolCall", "模型没有返回工具调用，画布操作未执行。"));
                upsertMessage(sessionId, { id: assistantId, role: "assistant", text: result.content || streamed || canvasT("videoCanvas.agent.noContent", "没有返回内容。") });
                addOnlineLog(canvasT("videoCanvas.agent.logLoopEnd", "Agent Tool Loop {{step}} 结束", { step: loop.step }), { reply: result.content });
            }
        } catch (error) {
            addOnlineLog(canvasT("videoCanvas.agent.logRequestFailed", "请求失败"), error instanceof Error ? error.message : error);
            appendMessage(sessionId, { id: nanoid(), role: "error", title: canvasT("videoCanvas.agent.opFailed", "操作失败"), text: error instanceof Error ? error.message : canvasT("videoCanvas.agent.opFailed", "操作失败") });
        } finally {
            setIsRunning(false);
        }
    };

    const continueOnlineToolLoop = async (sessionId: string, assistantId: string, messages: ResponseInputMessage[], result: { content: string; toolCalls: ResponseToolCall[] }, step: number) => {
        const toolResults = await executeOnlineToolCalls(sessionId, result.toolCalls);
        addOnlineLog(canvasT("videoCanvas.agent.logToolResults", "工具执行结果"), toolResults);
        appendMessage(sessionId, {
            id: nanoid(),
            role: "tool",
            title: canvasT("videoCanvas.agent.autoDoneTitle", "工具自动执行完成"),
            text: toolResults.map((item) => toolResultText(item.result)).join("\n"),
            detail: { status: "completed", step, toolCalls: result.toolCalls, results: toolResults },
        });
        await continueOnlineToolLoopAfterResults(sessionId, assistantId, messages, result.toolCalls, toolResults, step);
    };

    const continueOnlineToolLoopAfterResults = async (sessionId: string, assistantId: string, messages: ResponseInputMessage[], toolCalls: ResponseToolCall[], toolResults: OnlineExecutedToolCall[], step: number) => {
        const nextMessages = nextToolLoopMessages(messages, toolCalls, toolResults);
        if (step >= ONLINE_AGENT_MAX_STEPS) {
            upsertMessage(sessionId, { id: assistantId, role: "assistant", text: toolResults.map((item) => toolResultText(item.result)).join("\n") || canvasT("videoCanvas.agent.toolsExecutedDone", "工具已执行。") });
            addOnlineLog(canvasT("videoCanvas.agent.logLoopMax", "Agent Tool Loop 达到步数上限"), { maxSteps: ONLINE_AGENT_MAX_STEPS });
            return;
        }
        const requestConfig = { ...config, model: config.textModel || config.model };
        let streamed = "";
        const next = await requestToolResponse({ ...requestConfig, systemPrompt: "" }, nextMessages, ONLINE_AGENT_TOOLS, "auto", (text) => {
            streamed = text;
            if (text.trim()) upsertMessage(sessionId, { id: assistantId, role: "assistant", text });
        }, { promptCacheKey: canvasAgentPromptCacheKey(sessionId) });
        addOnlineLog(canvasT("videoCanvas.agent.logLoopReply", "Agent Tool Loop {{step}} 回复", { step: step + 1 }), next);
        if (next.toolCalls.length) {
            const writableCalls = next.toolCalls.filter(isWritableToolCall);
            if (confirmTools && writableCalls.length) {
                upsertMessage(sessionId, { id: assistantId, role: "assistant", text: next.content || streamed || canvasT("videoCanvas.agent.preparingWaitConfirm", "准备执行工具，等待确认。") });
                const toolMessageId = nanoid();
                pendingToolContextRef.current.set(toolMessageId, { messages: nextMessages, toolCalls: next.toolCalls, assistantId, step: step + 1 });
                appendMessage(sessionId, { id: toolMessageId, role: "tool", title: canvasT("videoCanvas.agent.toolConfirmTitle", "确认工具调用"), text: summarizeToolCalls(next.toolCalls), detail: { status: "pending", step: step + 1, toolCalls: next.toolCalls, impact: previewOnlineToolCalls(next.toolCalls, snapshotRef.current, config) } });
                addOnlineLog(canvasT("videoCanvas.agent.logAwaitConfirm", "等待用户确认"), next.toolCalls);
                return;
            }
            await continueOnlineToolLoop(sessionId, assistantId, nextMessages, next, step + 1);
            return;
        }
        upsertMessage(sessionId, { id: assistantId, role: "assistant", text: next.content || streamed || toolResults.map((item) => toolResultText(item.result)).join("\n") || canvasT("videoCanvas.agent.toolsExecutedDone", "工具已执行。") });
    };

    const executeOps = (ops: CanvasAgentOp[]) => {
        const beforeSnapshot = snapshotRef.current;
        const before = snapshotSignature(beforeSnapshot);
        const next = onApplyOps(ops);
        snapshotRef.current = next;
        const ranGeneration = ops.some((op) => op.type === "run_generation" && Boolean(op.nodeId));
        const changed = before !== snapshotSignature(next) || ranGeneration;
        const noopReason = changed ? "" : explainNoop(ops, beforeSnapshot);
        return { changed, ops, ranGeneration, noopReason, before: JSON.parse(before), after: JSON.parse(snapshotSignature(next)) };
    };

    const executeOnlineTool = async (sessionId: string, name: string, args: Record<string, unknown>): Promise<OnlineToolResult> => {
        const current = snapshotRef.current;
        try {
            if (name === "canvas_get_state") return { ok: true, message: describeCanvasSnapshot(current), data: compactSnapshot(current) };
            if (name === "canvas_export_snapshot") return { ok: true, message: describeCanvasSnapshot(current), data: compactSnapshot(current) };
            if (name === "canvas_get_selection") {
                const ids = new Set(current.selectedNodeIds || []);
                return { ok: true, message: canvasT("videoCanvas.agent.selectedNodesCount", "当前选中 {{count}} 个节点。", { count: ids.size }), data: { nodes: compactSnapshot({ ...current, nodes: current.nodes.filter((node) => ids.has(node.id)) }).nodes } };
            }
            if (name === "canvas_create_cinematic_session") {
                return {
                    ok: false,
                    message: "影视会话 Agent 未在 allo 服务端提供；请用客户端画布工具（创建节点 / 生成流程）完成同样目标。",
                };
            }
            const ops = onlineToolToOps(name, args, current, config);
            const result = executeOps(ops);
            return { ok: result.changed, message: result.changed ? summarizeCanvasAgentOps(ops) || canvasT("videoCanvas.agent.opsAppliedDone", "画布操作已执行。") : result.noopReason, data: result };
        } catch (error) {
            if (isAgentSessionPollingAbort(error)) throw error;
            return { ok: false, message: error instanceof Error ? error.message : canvasT("videoCanvas.agent.toolExecFailed", "工具执行失败") };
        }
    };

    const executeOnlineToolCall = async (sessionId: string, toolCall: ResponseToolCall): Promise<OnlineExecutedToolCall> => {
        try {
            const result = await executeOnlineTool(sessionId, toolCall.function.name, parseToolArguments(toolCall.function.arguments));
            return { toolCallId: toolCall.id, name: toolCall.function.name, result };
        } catch (error) {
            if (isAgentSessionPollingAbort(error)) throw error;
            return { toolCallId: toolCall.id, name: toolCall.function.name, result: { ok: false, message: error instanceof Error ? error.message : canvasT("videoCanvas.agent.toolParamError", "工具参数错误") } };
        }
    };

    const executeOnlineToolCalls = async (sessionId: string, toolCalls: ResponseToolCall[]) => {
        const results: OnlineExecutedToolCall[] = [];
        let stopped = false;
        for (const toolCall of toolCalls) {
            if (stopped) {
                results.push({ toolCallId: toolCall.id, name: toolCall.function.name, result: { ok: false, message: canvasT("videoCanvas.agent.prevFailedSkip", "前一个工具调用失败，未继续执行。") } });
                continue;
            }
            const result = await executeOnlineToolCall(sessionId, toolCall);
            results.push(result);
            if (!result.result.ok) stopped = true;
        }
        return results;
    };

    const approveOnlineTool = async (messageId: string) => {
        const message = getMessageById(messageId);
        const detail = objectDetail(message?.detail);
        const pendingContext = pendingToolContextRef.current.get(messageId);
        const toolCalls = pendingContext?.toolCalls || toolCallsFromDetail(detail);
        const previousMessages = pendingContext?.messages || [];
        const session = getSessionByMessageId(messageId);
        addOnlineLog(canvasT("videoCanvas.agent.approveTool", "批准工具"), { messageId, toolCalls });
        const assistantId = pendingContext?.assistantId || "";
        if (!session) return;
        if (!toolCalls.length || !previousMessages.length || !assistantId) {
            upsertMessage(session.id, { id: messageId, role: "tool", title: canvasT("videoCanvas.agent.toolExecFailed", "工具执行失败"), text: canvasT("videoCanvas.agent.contextIncomplete", "工具上下文不完整，无法执行。"), detail: { ...detail, status: "failed" } });
            return;
        }
        try {
            setIsRunning(true);
            const results = await executeOnlineToolCalls(session.id, toolCalls);
            addOnlineLog(canvasT("videoCanvas.agent.logToolResults", "工具执行结果"), results);
            upsertMessage(session.id, { id: messageId, role: "tool", title: canvasT("videoCanvas.agent.toolExecComplete", "工具执行完成"), text: results.map((item) => toolResultText(item.result)).join("\n"), detail: { ...detail, results, status: "completed" } });
            pendingToolContextRef.current.delete(messageId);
            await continueOnlineToolLoopAfterResults(session.id, assistantId, previousMessages, toolCalls, results, pendingContext?.step || Number(detail.step) || 1);
        } catch (error) {
            addOnlineLog(canvasT("videoCanvas.agent.continueRunFailed", "工具续跑失败"), error instanceof Error ? error.message : error);
            appendMessage(session.id, { id: nanoid(), role: "error", title: canvasT("videoCanvas.agent.opFailed", "操作失败"), text: error instanceof Error ? error.message : canvasT("videoCanvas.agent.opFailed", "操作失败") });
        } finally {
            setIsRunning(false);
        }
    };

    const rejectOnlineTool = (messageId: string) => {
        const session = getSessionByMessageId(messageId);
        addOnlineLog(canvasT("videoCanvas.agent.rejectTool", "拒绝工具"), { messageId });
        pendingToolContextRef.current.delete(messageId);
        if (session) upsertMessage(session.id, { id: messageId, role: "tool", title: canvasT("videoCanvas.agent.rejectedTitle", "已拒绝执行"), text: canvasT("videoCanvas.agent.rejectedText", "工具调用已取消"), detail: { ...objectDetail(session.messages.find((item) => item.id === messageId)?.detail), status: "rejected" } });
    };

    return { isRunning, onlineLogs, addOnlineLog, clearOnlineLogs: () => setOnlineLogs([]), sendMessage, approveOnlineTool, rejectOnlineTool, executeOps, setIsRunning };
}

function toolResultText(result: OnlineToolResult) {
    return result.message;
}

function snapshotSignature(snapshot: CanvasAgentSnapshot) {
    return JSON.stringify({ nodes: snapshot.nodes, connections: snapshot.connections, selectedNodeIds: snapshot.selectedNodeIds, viewport: snapshot.viewport });
}

function explainNoop(ops: CanvasAgentOp[], snapshot: CanvasAgentSnapshot) {
    if (!ops.length) return canvasT("videoCanvas.agent.noopNone", "模型没有返回可执行的画布操作。");
    const nodeIds = new Set(snapshot.nodes.map((node) => node.id));
    const connectionIds = new Set(snapshot.connections.map((conn) => conn.id));
    const deleteConnectionOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "delete_connections" }> => op.type === "delete_connections");
    const connectOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "connect_nodes" }> => op.type === "connect_nodes");
    const deleteNodeOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "delete_node" }> => op.type === "delete_node");
    const updateOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "update_node" }> => op.type === "update_node");
    const selectOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "select_nodes" }> => op.type === "select_nodes");
    const generationOps = ops.filter((op): op is Extract<CanvasAgentOp, { type: "run_generation" }> => op.type === "run_generation");
    if (deleteConnectionOps.length && !snapshot.connections.length) return canvasT("videoCanvas.agent.noopNoConnectionsDelete", "画布当前没有连线可删除。");
    if (deleteConnectionOps.length && deleteConnectionOps.every((op) => !op.all && [...(op.ids || []), ...(op.id ? [op.id] : [])].every((id) => !connectionIds.has(id)))) return "没有找到要删除的连线。";
    if (connectOps.length && connectOps.every((op) => snapshot.connections.some((conn) => conn.fromNodeId === op.fromNodeId && conn.toNodeId === op.toNodeId))) return canvasT("videoCanvas.agent.noopConnectionsExist", "这些节点已经存在对应连线，无需重复连接。");
    if (connectOps.length && connectOps.every((op) => !nodeIds.has(op.fromNodeId) || !nodeIds.has(op.toNodeId))) return canvasT("videoCanvas.agent.noopNodesMissingConnect", "没有找到要连接的节点。");
    if (deleteNodeOps.length && deleteNodeOps.every((op) => op.nodeType === CanvasNodeType.Config) && !snapshot.nodes.some((node) => node.type === CanvasNodeType.Config)) return canvasT("videoCanvas.agent.noopConfigMissingDelete", "画布当前没有生成配置节点可删除。");
    if (deleteNodeOps.length && deleteNodeOps.every((op) => [...(op.ids || []), ...(op.id ? [op.id] : [])].every((id) => !nodeIds.has(id)))) return canvasT("videoCanvas.agent.noopNodesMissingDelete", "没有找到要删除的节点。");
    if (updateOps.length && updateOps.every((op) => !nodeIds.has(op.id))) return canvasT("videoCanvas.agent.noopNodesMissingUpdate", "没有找到要更新的节点。");
    if (selectOps.length && selectOps.every((op) => !(op.ids || []).some((id) => nodeIds.has(id)))) return canvasT("videoCanvas.agent.noopNodesMissingSelect", "没有找到要选择的节点。");
    if (generationOps.length && generationOps.every((op) => !nodeIds.has(op.nodeId))) return canvasT("videoCanvas.agent.noopNodesMissingGenerate", "没有找到要触发生成的节点。");
    if (ops.every((op) => op.type === "set_viewport")) return canvasT("videoCanvas.agent.noopViewportAlready", "视图已经是目标状态。");
    if (selectOps.length && selectOps.every((op) => JSON.stringify(op.ids || []) === JSON.stringify(snapshot.selectedNodeIds))) return canvasT("videoCanvas.agent.noopSelectionAlready", "选区已经是目标状态。");
    return canvasT("videoCanvas.agent.noopExecutedNoChange", "工具已执行，但画布状态没有变化；请在日志 tab 查看工具参数和执行前后状态。");
}

function nextToolLoopMessages(messages: ResponseInputMessage[], toolCalls: ResponseToolCall[], toolResults: OnlineExecutedToolCall[]): ResponseInputMessage[] {
    return [
        ...messages,
        ...toolCalls.map(toolCallToResponseInput),
        ...toolResults.map((item) => ({ role: "tool" as const, tool_call_id: item.toolCallId, content: JSON.stringify(item.result) })),
    ];
}

async function buildToolAgentMessages(snapshot: CanvasAgentSnapshot, history: CanvasAssistantMessage[], userMessage: CanvasAssistantMessage): Promise<ResponseInputMessage[]> {
    const refs = userMessage.references || [];
    return [
        { role: "system", content: ONLINE_AGENT_PROMPT },
        ...history
            .filter((message) => message.role === "user" || message.role === "assistant" || message.role === "system")
            .slice(-8)
            .map((message): ResponseInputMessage => ({ role: message.role as "system" | "user" | "assistant", content: message.text })),
        {
            role: "user",
            content: [
                ...refs.flatMap((item) => (item.text ? [{ type: "text" as const, text: `选中节点 ${item.title}：${item.text}` }] : [])),
                { type: "text", text: `当前画布：${JSON.stringify(compactSnapshot(snapshot))}\n\n用户需求：${userMessage.text}` },
                ...(await Promise.all(refs.filter((item) => item.dataUrl).map(async (item) => ({ type: "image_url" as const, image_url: { url: await imageToDataUrl(item) } })))),
            ],
        },
    ];
}
