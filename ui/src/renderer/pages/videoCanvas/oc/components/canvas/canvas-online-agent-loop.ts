import { useRef, useState, type MutableRefObject } from "react";
import { nanoid } from "nanoid";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { compactCanvasAgentSnapshot as compactSnapshot } from "@oc/lib/canvas/canvas-agent-snapshot-compact";
import { isAgentSessionPollingAbort } from "@oc/lib/canvas/canvas-agent-session";
import { canvasAgentPostconditionMessage, canvasAgentStateHashBlocksWrite, verifyCanvasAgentOps, type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { buildCanvasAgentContext, findCanvasAgentNodes, getCanvasAgentConnection, getCanvasAgentGenerationTasks, getCanvasAgentNode, getCanvasAgentResources, validateCanvasAgentOps } from "@oc/lib/canvas/canvas-agent-context";
import { parseCanvasAgentMentionTokens, resolveCanvasAgentNodeIds } from "@oc/lib/canvas/canvas-agent-ids";
import { compileCanvasRunOps, critiqueCanvasOutputs, inspectCanvasIntent, isCanvasApplyNeedsGraphError, proposeCanvasApply } from "@oc/lib/canvas/canvas-agent-intent";
import { buildCanvasAgentObservation, CANVAS_AGENT_CODES, compactWriteToolData, observationPromptBlock } from "@oc/lib/canvas/canvas-agent-observation";
import { waitCanvasAgentGeneration } from "@oc/lib/canvas/canvas-agent-wait";
import { collectCanvasSkills } from "@oc/lib/canvas/canvas-skill-mentions";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { requestCanvasAgentTurn, type CanvasAgentInputMessage as ResponseInputMessage, type CanvasAgentToolCall as ResponseToolCall } from "@oc/lib/canvas/canvas-agent-llm";
import { canvasHarness, CANVAS_AGENT_INCOMPLETE_NUDGE } from "@oc/lib/canvas/canvas-agent-harness";
import { imageToDataUrl } from "@oc/services/image-storage";
import { useConfigStore, type AiConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType, type CanvasAssistantMessage, type CanvasAssistantReference, type CanvasAssistantSession } from "@oc/types/canvas";
import {
    dropPendingOnlineToolContext,
    lastUserMessage,
    pendingToolDetail,
    pendingToolMessageHistory,
    stashPendingOnlineToolContext,
    resolvePendingOnlineToolContext,
} from "./canvas-online-agent-pending";
import {
    ONLINE_AGENT_TOOLS,
    ONLINE_READ_TOOLS,
    describeCanvasSnapshot,
    describeOnlineToolActivity,
    describeOnlineToolProgress,
    inspectCanvasOps,
    isWritableToolCall,
    objectDetail,
    onlineToolToOps,
    parseToolArguments,
    previewOnlineToolCalls,
    requireOps,
    requireString,
    summarizeToolCalls,
    toolCallToResponseInput,
    type OnlineToolResult,
} from "./canvas-online-agent-tools";

export type OnlineAgentLog = { id: string; time: string; title: string; data?: unknown };
export type SendOnlineAgentOptions = {
    modelContext?: string;
    meta?: string;
    skipConfirm?: boolean;
    onUnready?: "settings" | "wait";
};
type OnlineLoopContext = { step: number; skipConfirm?: boolean };
type OnlineExecutedToolCall = { toolCallId: string; name: string; result: OnlineToolResult };
export type OnlineAgentLoopOptions = {
    snapshotRef: MutableRefObject<CanvasAgentSnapshot>;
    config: AiConfig;
    confirmTools: boolean;
    selectedReferences: CanvasAssistantReference[];
    activeSession: CanvasAssistantSession | null;
    onApplyOps: (ops?: CanvasAgentOp[]) => CanvasAgentSnapshot;
    onExtractFrames?: (nodeId: string, timesMs: number[]) => Promise<{ createdNodeIds: string[]; message: string }>;
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
    onExtractFrames,
    appendMessage,
    upsertMessage,
    getMessageById,
    getSessionByMessageId,
    activateSession,
}: OnlineAgentLoopOptions) {
    const isAiConfigReady = useConfigStore((state) => state.isAiConfigReady);
    const [isRunning, setIsRunning] = useState(false);
    const [agentActivity, setAgentActivity] = useState<string | null>(null);
    const [onlineLogs, setOnlineLogs] = useState<OnlineAgentLog[]>([]);
    const previousSnapshotRef = useRef<CanvasAgentSnapshot | null>(null);
    const inspectedNodeIdsRef = useRef(new Set<string>());
    const createdNodeIdsRef = useRef(new Set<string>());

    const addOnlineLog = (title: string, data?: unknown) => setOnlineLogs((prev) => [{ id: nanoid(), time: new Date().toLocaleTimeString(), title, data }, ...prev].slice(0, 80));

    const sendMessage = async (text: string, history: CanvasAssistantMessage[], savedReferences?: CanvasAssistantReference[], options?: SendOnlineAgentOptions) => {
        const requestConfig = { ...config, model: config.textModel || config.model };
        if (!isAiConfigReady(requestConfig, requestConfig.model)) {
            if (options?.onUnready !== "wait") navigateToSettings({ continueCreation: true });
            return false;
        }

        const session = activeSession || createSession();
        if (!activeSession) activateSession(session);

        const refs = savedReferences || selectedReferences;
        const userMessage: CanvasAssistantMessage = { id: nanoid(), role: "user", text, references: refs, modelContext: options?.modelContext, meta: options?.meta };
        const assistantId = nanoid();
        inspectedNodeIdsRef.current = new Set(snapshotRef.current.nodes.map((node) => node.id));
        refs.forEach((item) => inspectedNodeIdsRef.current.add(item.id));
        resolveCanvasAgentNodeIds(snapshotRef.current, parseCanvasAgentMentionTokens(text)).ids.forEach((id) => inspectedNodeIdsRef.current.add(id));
        createdNodeIdsRef.current = new Set();
        previousSnapshotRef.current = snapshotRef.current;
        appendMessage(session.id, userMessage);
        addOnlineLog(canvasT("videoCanvas.agent.logSendRequest", "发送请求"), { text, selectedNodeIds: snapshotRef.current.selectedNodeIds, nodeCount: snapshotRef.current.nodes.length, connectionCount: snapshotRef.current.connections.length });
        setAgentActivity(canvasT("videoCanvas.agent.activityUnderstanding", "正在理解任务…"));
        setIsRunning(true);
        void runOnlineAgentStep(session.id, assistantId, history, userMessage, { step: 1, skipConfirm: options?.skipConfirm });
        return true;
    };

    const runOnlineAgentStep = async (sessionId: string, assistantId: string, history: CanvasAssistantMessage[], userMessage: CanvasAssistantMessage, loop: OnlineLoopContext) => {
        try {
            setIsRunning(true);
            setAgentActivity(loop.step === 1
                ? canvasT("videoCanvas.agent.activityUnderstanding", "正在理解任务…")
                : canvasT("videoCanvas.agent.activityPlanning", "正在规划下一步…"));
            const messages = await buildToolAgentMessages(snapshotRef.current, history, userMessage, previousSnapshotRef.current);
            const toolChoice = loop.step === 1 ? canvasHarness.firstTurnToolChoice() : "auto";
            addOnlineLog(canvasT("videoCanvas.agent.logLoopStart", "Agent Tool Loop {{step}} 开始", { step: loop.step }), { toolChoice });
            let streamed = "";
            const requestConfig = { ...config, model: config.textModel || config.model, systemPrompt: "" };
            const result = await requestCanvasAgentTurn(requestConfig, messages, ONLINE_AGENT_TOOLS, toolChoice, {
                onDelta: (text) => {
                    streamed = text;
                    if (text.trim()) upsertMessage(sessionId, { id: assistantId, role: "assistant", text });
                },
            });
            addOnlineLog(canvasT("videoCanvas.agent.logModelToolReply", "模型工具回复"), result);
            const decision = canvasHarness.decideAfterModel({
                step: loop.step,
                toolCallCount: result.toolCalls.length,
                writableCallCount: result.toolCalls.filter(isWritableToolCall).length,
                confirmTools,
                skipConfirm: loop.skipConfirm,
                incomplete: buildCanvasAgentObservation(snapshotRef.current, previousSnapshotRef.current).incomplete,
            });
            if (decision.action === "await_confirm") {
                upsertMessage(sessionId, { id: assistantId, role: "assistant", text: result.content || streamed || canvasT("videoCanvas.agent.preparingWaitConfirm", "准备执行工具，等待确认。") });
                const toolMessageId = nanoid();
                stashPendingOnlineToolContext(toolMessageId, { messages, toolCalls: result.toolCalls, assistantId, step: loop.step });
                const toolMessage: CanvasAssistantMessage = { id: toolMessageId, role: "tool", title: canvasT("videoCanvas.agent.toolConfirmTitle", "确认工具调用"), text: summarizeToolCalls(result.toolCalls), detail: pendingToolDetail({ status: "pending", impact: previewOnlineToolCalls(result.toolCalls, snapshotRef.current, config) }, { assistantId, step: loop.step, toolCalls: result.toolCalls }) };
                appendMessage(sessionId, toolMessage);
                addOnlineLog(canvasT("videoCanvas.agent.logAwaitConfirm", "等待用户确认"), result.toolCalls);
                return;
            }
            if (decision.action === "run_tools") {
                await continueOnlineToolLoop(sessionId, assistantId, messages, result, loop.step, loop.skipConfirm);
                return;
            }
            if (!result.content.trim()) throw new Error(canvasT("videoCanvas.agent.errNoToolCall", "模型没有返回工具调用，画布操作未执行。"));
            upsertMessage(sessionId, { id: assistantId, role: "assistant", text: result.content || streamed || canvasT("videoCanvas.agent.noContent", "没有返回内容。") });
            addOnlineLog(canvasT("videoCanvas.agent.logLoopEnd", "Agent Tool Loop {{step}} 结束", { step: loop.step }), { reply: result.content });
        } catch (error) {
            addOnlineLog(canvasT("videoCanvas.agent.logRequestFailed", "请求失败"), error instanceof Error ? error.message : error);
            appendMessage(sessionId, { id: nanoid(), role: "error", title: canvasT("videoCanvas.agent.opFailed", "操作失败"), text: error instanceof Error ? error.message : canvasT("videoCanvas.agent.opFailed", "操作失败") });
        } finally {
            setAgentActivity(null);
            setIsRunning(false);
        }
    };

    const continueOnlineToolLoop = async (sessionId: string, assistantId: string, messages: ResponseInputMessage[], result: { content: string; toolCalls: ResponseToolCall[] }, step: number, skipConfirm?: boolean) => {
        const toolResults = await executeOnlineToolCalls(sessionId, result.toolCalls);
        addOnlineLog(canvasT("videoCanvas.agent.logToolResults", "工具执行结果"), toolResults);
        setAgentActivity(canvasT("videoCanvas.agent.activityPlanning", "正在规划下一步…"));
        await continueOnlineToolLoopAfterResults(sessionId, assistantId, messages, result.toolCalls, toolResults, step, skipConfirm);
    };

    const continueOnlineToolLoopAfterResults = async (sessionId: string, assistantId: string, messages: ResponseInputMessage[], toolCalls: ResponseToolCall[], toolResults: OnlineExecutedToolCall[], step: number, skipConfirm?: boolean) => {
        const nextMessages = refreshObservationMessages(nextToolLoopMessages(messages, toolCalls, toolResults), snapshotRef.current, previousSnapshotRef.current);
        const afterTools = canvasHarness.decideAfterTools(step);
        if (afterTools.action === "hard_stop") {
            const observation = buildCanvasAgentObservation(snapshotRef.current, previousSnapshotRef.current);
            upsertMessage(sessionId, { id: assistantId, role: "assistant", text: [`已达步数上限（${canvasHarness.maxSteps}）。`, observationPromptBlock(observation), ...toolResults.map((item) => toolResultText(item.result))].filter(Boolean).join("\n") });
            addOnlineLog(canvasT("videoCanvas.agent.logLoopMax", "Agent Tool Loop 达到步数上限"), { maxSteps: canvasHarness.maxSteps, incomplete: observation.incomplete });
            return;
        }
        const requestConfig = { ...config, model: config.textModel || config.model, systemPrompt: "" };
        setAgentActivity(canvasT("videoCanvas.agent.activityPlanning", "正在规划下一步…"));
        let streamed = "";
        const next = await requestCanvasAgentTurn(requestConfig, nextMessages, ONLINE_AGENT_TOOLS, afterTools.toolChoice, {
            onDelta: (text) => {
                streamed = text;
                if (text.trim()) upsertMessage(sessionId, { id: assistantId, role: "assistant", text });
            },
        });
        addOnlineLog(canvasT("videoCanvas.agent.logLoopReply", "Agent Tool Loop {{step}} 回复", { step: step + 1 }), next);
        const observation = buildCanvasAgentObservation(snapshotRef.current, previousSnapshotRef.current);
        const decision = canvasHarness.decideAfterModel({
            step: step + 1,
            toolCallCount: next.toolCalls.length,
            writableCallCount: next.toolCalls.filter(isWritableToolCall).length,
            confirmTools,
            skipConfirm,
            incomplete: observation.incomplete,
        });
        if (decision.action === "force_tool") {
            addOnlineLog("生成队列未空，harness 继续循环", observation);
            const nudged = refreshObservationMessages([
                ...nextMessages,
                { role: "user", content: `${observationPromptBlock(observation)}\n${CANVAS_AGENT_INCOMPLETE_NUDGE}` },
            ], snapshotRef.current, previousSnapshotRef.current);
            const forced = await requestCanvasAgentTurn(requestConfig, nudged, ONLINE_AGENT_TOOLS, "required", {
                onDelta: (text) => {
                    streamed = text;
                    if (text.trim()) upsertMessage(sessionId, { id: assistantId, role: "assistant", text });
                },
            });
            if (forced.toolCalls.length) {
                await continueOnlineToolLoop(sessionId, assistantId, nudged, forced, step + 1, skipConfirm);
                return;
            }
            upsertMessage(sessionId, { id: assistantId, role: "assistant", text: next.content || streamed || toolResults.map((item) => toolResultText(item.result)).join("\n") || canvasT("videoCanvas.agent.toolsExecutedDone", "工具已执行。") });
            return;
        }
        if (decision.action === "await_confirm") {
            upsertMessage(sessionId, { id: assistantId, role: "assistant", text: next.content || streamed || canvasT("videoCanvas.agent.preparingWaitConfirm", "准备执行工具，等待确认。") });
            const toolMessageId = nanoid();
            stashPendingOnlineToolContext(toolMessageId, { messages: nextMessages, toolCalls: next.toolCalls, assistantId, step: step + 1 });
            appendMessage(sessionId, { id: toolMessageId, role: "tool", title: canvasT("videoCanvas.agent.toolConfirmTitle", "确认工具调用"), text: summarizeToolCalls(next.toolCalls), detail: pendingToolDetail({ status: "pending", impact: previewOnlineToolCalls(next.toolCalls, snapshotRef.current, config) }, { assistantId, step: step + 1, toolCalls: next.toolCalls }) });
            addOnlineLog(canvasT("videoCanvas.agent.logAwaitConfirm", "等待用户确认"), next.toolCalls);
            return;
        }
        if (decision.action === "run_tools") {
            await continueOnlineToolLoop(sessionId, assistantId, nextMessages, next, step + 1, skipConfirm);
            return;
        }
        upsertMessage(sessionId, { id: assistantId, role: "assistant", text: next.content || streamed || toolResults.map((item) => toolResultText(item.result)).join("\n") || canvasT("videoCanvas.agent.toolsExecutedDone", "工具已执行。") });
    };

    const executeOps = (ops: CanvasAgentOp[]) => {
        const beforeSnapshot = snapshotRef.current;
        const validation = validateCanvasAgentOps(beforeSnapshot, ops);
        if (!validation.ok) {
            throw new Error(`画布操作校验失败：${validation.issues.filter((item) => item.severity === "error").map((item) => item.message).join("；")}`);
        }
        const before = snapshotSignature(beforeSnapshot);
        const next = onApplyOps(ops);
        snapshotRef.current = next;
        const verification = verifyCanvasAgentOps(beforeSnapshot, next, ops);
        const ranGeneration = ops.some((op) => op.type === "run_generation" && Boolean(op.nodeId));
        const changed = before !== snapshotSignature(next) || ranGeneration || verification.changed;
        const noopReason = changed ? "" : explainNoop(ops, beforeSnapshot);
        const data = compactWriteToolData(verification, next, previousSnapshotRef.current);
        previousSnapshotRef.current = next;
        return { ...verification, verification, snapshot: next, ops, noopReason, data, before: undefined, after: undefined };
    };

    const executeOnlineTool = async (sessionId: string, name: string, args: Record<string, unknown>): Promise<OnlineToolResult> => {
        const current = snapshotRef.current;
        try {
            const expectedRevision = typeof args.expectedRevision === "number" ? args.expectedRevision : undefined;
            if (expectedRevision !== undefined && expectedRevision !== (current.revision ?? 0)) return { ok: false, message: "画布 revision 已变化，请重新 canvas_inspect 后再写入。" };
            const expectedStateHash = typeof args.expectedStateHash === "string" ? args.expectedStateHash : "";
            if (canvasAgentStateHashBlocksWrite(expectedStateHash, buildCanvasAgentContext(current).stateHash, name, args)) return { ok: false, message: "画布状态已变化，请重新 canvas_inspect 后再写入。" };
            if (name === "canvas_list_skills") {
                const skills = collectCanvasSkills(current.nodes);
                const data = skills.map((skill) => ({ skillId: skill.skill_id, name: skill.skill_name, description: skill.description, tag: skill.tag }));
                return { ok: true, message: data.length ? "已列出当前可用技能。" : "当前画布没有技能节点。", data };
            }
            if (name === "canvas_get_skill") {
                const skillId = typeof args.skillId === "string" ? args.skillId : "";
                const nameQuery = typeof args.name === "string" ? args.name.trim().toLocaleLowerCase() : "";
                const skill = collectCanvasSkills(current.nodes).find((item) => item.skill_id === skillId || item.skill_name.toLocaleLowerCase() === nameQuery);
                if (!skill) return { ok: false, message: "未找到画布技能，请先调用 canvas_list_skills。" };
                return {
                    ok: true,
                    message: `已按需加载技能「${skill.skill_name}」。`,
                    data: { skillId: skill.skill_id, name: skill.skill_name, description: skill.description, instruction: skill.instruction || skill.description, version: skill.update_time },
                };
            }
            if (name === "canvas_inspect" || name === "canvas_get_state" || name === "canvas_get_context" || name === "canvas_export_snapshot" || name === "canvas_get_selection") {
                rememberSnapshotNodes(current, inspectedNodeIdsRef.current);
                const data = inspectCanvasIntent(current, name === "canvas_inspect" ? args : { focus: "graph" }, previousSnapshotRef.current);
                previousSnapshotRef.current = current;
                return { ok: true, message: describeCanvasSnapshot(current), data };
            }
            if (name === "canvas_propose") {
                const data = proposeCanvasApply(args as Parameters<typeof proposeCanvasApply>[0], current, config);
                return { ok: true, message: data.plan.title, data };
            }
            if (name === "canvas_critique") {
                const nodeIds = Array.isArray(args.nodeIds) ? args.nodeIds.filter((id): id is string => typeof id === "string") : undefined;
                const data = critiqueCanvasOutputs(current, nodeIds);
                return { ok: data.ok, message: data.issues.length ? data.issues.map((item) => item.message).join("；") : "产物检查通过。", data };
            }
            if (name === "canvas_find_nodes") {
                const data = findCanvasAgentNodes(current, args as Parameters<typeof findCanvasAgentNodes>[1]);
                data.nodes.forEach((node) => inspectedNodeIdsRef.current.add(node.id));
                return { ok: true, message: "已按条件检索真实节点。", data };
            }
            if (name === "canvas_get_node") {
                const data = getCanvasAgentNode(current, { id: requireString(args.id, "id") });
                if (data.found && data.id) inspectedNodeIdsRef.current.add(data.id);
                return { ok: true, message: data.found ? "已精确读取节点。" : "未找到指定节点。", data };
            }
            if (name === "canvas_get_connection") {
                const data = getCanvasAgentConnection(current, { id: requireString(args.id, "id") });
                return { ok: true, message: data.found ? "已精确读取连线。" : "未找到指定连线。", data };
            }
            if (name === "canvas_get_generation_tasks") return { ok: true, message: "已读取画布生成任务观察状态。", data: getCanvasAgentGenerationTasks(current, args as Parameters<typeof getCanvasAgentGenerationTasks>[1]) };
            if (name === "canvas_wait_generation" || name === "canvas_run") {
                const nodeIds = Array.isArray(args.nodeIds) ? args.nodeIds.filter((id): id is string => typeof id === "string") : undefined;
                const timeoutMs = typeof args.timeoutMs === "number" ? args.timeoutMs : undefined;
                if (name === "canvas_run") {
                    rememberWriteTargets(name, args, current, inspectedNodeIdsRef.current, createdNodeIdsRef.current);
                    const ops = compileCanvasRunOps(current, nodeIds);
                    const result = executeOps(ops);
                    result.createdNodeIds.forEach((id) => {
                        createdNodeIdsRef.current.add(id);
                        inspectedNodeIdsRef.current.add(id);
                    });
                    if (args.wait === false) {
                        return { ok: result.ok, message: result.changed ? canvasAgentPostconditionMessage(result) : result.noopReason, data: result.data };
                    }
                    const waitResult = await waitCanvasAgentGeneration(() => snapshotRef.current, { nodeIds: ops.map((op) => op.nodeId), timeoutMs });
                    const observation = buildCanvasAgentObservation(snapshotRef.current, previousSnapshotRef.current);
                    previousSnapshotRef.current = snapshotRef.current;
                    const code = waitResult.timedOut ? CANVAS_AGENT_CODES.WAIT_TIMEOUT : observation.failed.length ? CANVAS_AGENT_CODES.GENERATION_FAILED : CANVAS_AGENT_CODES.OK;
                    const message = waitResult.timedOut
                        ? canvasT("videoCanvas.agent.waitTimedOut", "等待生成超时，仍有 {{count}} 个任务未完成。", { count: waitResult.pendingCount })
                        : canvasT("videoCanvas.agent.waitDone", "生成任务已到达终态。");
                    return { ok: true, message, data: { code, submitted: result.data, wait: waitResult, observation } };
                }
                const result = await waitCanvasAgentGeneration(() => snapshotRef.current, { nodeIds, timeoutMs });
                const message = result.timedOut
                    ? canvasT("videoCanvas.agent.waitTimedOut", "等待生成超时，仍有 {{count}} 个任务未完成。", { count: result.pendingCount })
                    : canvasT("videoCanvas.agent.waitDone", "生成任务已到达终态。");
                return { ok: true, message, data: { code: result.timedOut ? CANVAS_AGENT_CODES.WAIT_TIMEOUT : CANVAS_AGENT_CODES.OK, ...result } };
            }
            if (name === "canvas_get_resources") return { ok: true, message: "已读取画布资源清单。", data: getCanvasAgentResources(current, args as Parameters<typeof getCanvasAgentResources>[1]) };
            if (name === "canvas_validate_ops") {
                const inspected = inspectCanvasOps(args.ops);
                if (inspected.issues.length) {
                    return {
                        ok: false,
                        message: `校验未通过：${inspected.issues.join("；")}`,
                        data: { ok: false, code: CANVAS_AGENT_CODES.VALIDATE_FAILED, issues: inspected.issues.map((message, index) => ({ index, severity: "error" as const, message })), parsedCount: inspected.ops.length },
                    };
                }
                const result = validateCanvasAgentOps(current, inspected.ops);
                return { ok: result.ok, message: result.ok ? "操作校验通过。" : `校验未通过：${result.issues.filter((item) => item.severity === "error").map((item) => item.message).join("；")}`, data: { ...result, code: result.ok ? CANVAS_AGENT_CODES.OK : CANVAS_AGENT_CODES.VALIDATE_FAILED } };
            }
            if (name === "canvas_export_snapshot") {
                rememberSnapshotNodes(current, inspectedNodeIdsRef.current);
                return { ok: true, message: describeCanvasSnapshot(current), data: compactSnapshot(current) };
            }
            if (name === "canvas_get_selection") {
                const ids = new Set(current.selectedNodeIds || []);
                ids.forEach((id) => inspectedNodeIdsRef.current.add(id));
                return { ok: true, message: canvasT("videoCanvas.agent.selectedNodesCount", "当前选中 {{count}} 个节点。", { count: ids.size }), data: { nodes: compactSnapshot({ ...current, nodes: current.nodes.filter((node) => ids.has(node.id)) }).nodes } };
            }
            if (name === "canvas_extract_frames") {
                const ops = onlineToolToOps(name, args, current, config);
                const extractOp = ops.find((op): op is Extract<CanvasAgentOp, { type: "extract_frames" }> => op.type === "extract_frames");
                if (!extractOp) return { ok: false, message: canvasT("videoCanvas.agent.extractMissing", "缺少可提取的视频节点。") };
                if (!onExtractFrames) return { ok: false, message: canvasT("videoCanvas.agent.extractUnavailable", "当前画布未接入画面提取。") };
                const timesMs = extractOp.timesMs.length ? extractOp.timesMs : defaultExtractTimes(current, extractOp.nodeId);
                const extracted = await onExtractFrames(extractOp.nodeId, timesMs);
                extracted.createdNodeIds.forEach((id) => {
                    createdNodeIdsRef.current.add(id);
                    inspectedNodeIdsRef.current.add(id);
                });
                return { ok: true, message: extracted.message, data: extracted };
            }
            rememberWriteTargets(name, args, current, inspectedNodeIdsRef.current, createdNodeIdsRef.current);
            const ops = onlineToolToOps(name, args, current, config);
            const result = executeOps(ops);
            result.createdNodeIds.forEach((id) => {
                createdNodeIdsRef.current.add(id);
                inspectedNodeIdsRef.current.add(id);
            });
            rememberSnapshotNodes(snapshotRef.current, inspectedNodeIdsRef.current);
            if ((name === "canvas_apply" || name === "canvas_create_workflow") && (args.run === true || args.autoRun === true)) {
                const waitIds = result.generation.map((item) => item.nodeId).filter(Boolean);
                if (waitIds.length) {
                    const waitResult = await waitCanvasAgentGeneration(() => snapshotRef.current, { nodeIds: waitIds });
                    const observation = buildCanvasAgentObservation(snapshotRef.current, previousSnapshotRef.current);
                    previousSnapshotRef.current = snapshotRef.current;
                    return {
                        ok: true,
                        message: waitResult.timedOut
                            ? canvasT("videoCanvas.agent.waitTimedOut", "等待生成超时，仍有 {{count}} 个任务未完成。", { count: waitResult.pendingCount })
                            : result.changed ? canvasAgentPostconditionMessage(result) : result.noopReason,
                        data: { ...(result.data || {}), wait: waitResult, observation, code: waitResult.timedOut ? CANVAS_AGENT_CODES.WAIT_TIMEOUT : result.data?.code },
                    };
                }
            }
            return { ok: result.ok, message: result.changed ? canvasAgentPostconditionMessage(result) : result.noopReason, data: result.data || result };
        } catch (error) {
            if (isAgentSessionPollingAbort(error)) throw error;
            const message = error instanceof Error ? error.message : canvasT("videoCanvas.agent.toolExecFailed", "工具执行失败");
            return {
                ok: false,
                message,
                data: isCanvasApplyNeedsGraphError(error) ? { code: CANVAS_AGENT_CODES.APPLY_NEEDS_NODES } : undefined,
            };
        }
    };

    const executeOnlineToolCall = async (sessionId: string, toolCall: ResponseToolCall): Promise<OnlineExecutedToolCall> => {
        const runningId = nanoid();
        const name = toolCall.function.name;
        setAgentActivity(describeOnlineToolProgress(name));
        appendMessage(sessionId, {
            id: runningId,
            role: "tool",
            title: describeOnlineToolActivity(name),
            text: canvasT("videoCanvas.agent.toolRunning", "正在执行…"),
            detail: { status: "running", name, toolCalls: [toolCall] },
        });
        try {
            const result = await executeOnlineTool(sessionId, name, parseToolArguments(toolCall.function.arguments));
            const needsGraph = !result.ok && (result.data as { code?: string } | undefined)?.code === CANVAS_AGENT_CODES.APPLY_NEEDS_NODES;
            upsertMessage(sessionId, {
                id: runningId,
                role: "tool",
                title: describeOnlineToolActivity(name),
                text: result.message,
                detail: { status: result.ok ? "completed" : needsGraph ? "noop" : "failed", name, toolCalls: [toolCall], result },
            });
            return { toolCallId: toolCall.id, name, result };
        } catch (error) {
            if (isAgentSessionPollingAbort(error)) throw error;
            const message = error instanceof Error ? error.message : canvasT("videoCanvas.agent.toolParamError", "工具参数错误");
            upsertMessage(sessionId, {
                id: runningId,
                role: "tool",
                title: describeOnlineToolActivity(name),
                text: message,
                detail: { status: "failed", name, toolCalls: [toolCall], result: { ok: false, message } },
            });
            return { toolCallId: toolCall.id, name, result: { ok: false, message } };
        }
    };

    const executeOnlineToolCalls = async (sessionId: string, toolCalls: ResponseToolCall[]) => {
        const allReads = toolCalls.length > 1 && toolCalls.every((call) => ONLINE_READ_TOOLS.has(call.function.name));
        if (allReads) return Promise.all(toolCalls.map((call) => executeOnlineToolCall(sessionId, call)));
        const results: OnlineExecutedToolCall[] = [];
        for (const toolCall of toolCalls) {
            results.push(await executeOnlineToolCall(sessionId, toolCall));
        }
        return results;
    };

    const approveOnlineTool = async (messageId: string) => {
        const message = getMessageById(messageId);
        const detail = objectDetail(message?.detail);
        const session = getSessionByMessageId(messageId);
        const pendingContext = resolvePendingOnlineToolContext(messageId, detail, session);
        const toolCalls = pendingContext?.toolCalls || [];
        addOnlineLog(canvasT("videoCanvas.agent.approveTool", "批准工具"), { messageId, toolCalls });
        if (!session) return;
        const previousMessages = pendingContext?.messages.length
            ? pendingContext.messages
            : await rebuildPendingToolMessages(session);
        const assistantId = pendingContext?.assistantId || "";
        if (!toolCalls.length || !previousMessages.length || !assistantId) {
            upsertMessage(session.id, { id: messageId, role: "tool", title: canvasT("videoCanvas.agent.toolExecFailed", "工具执行失败"), text: canvasT("videoCanvas.agent.contextIncomplete", "工具上下文不完整，无法执行。"), detail: { ...detail, status: "failed" } });
            return;
        }
        try {
            setIsRunning(true);
            const results = await executeOnlineToolCalls(session.id, toolCalls);
            addOnlineLog(canvasT("videoCanvas.agent.logToolResults", "工具执行结果"), results);
            upsertMessage(session.id, { id: messageId, role: "tool", title: canvasT("videoCanvas.agent.toolExecComplete", "工具执行完成"), text: summarizeToolCalls(toolCalls), detail: { ...detail, results, status: "completed" } });
            dropPendingOnlineToolContext(messageId);
            setAgentActivity(canvasT("videoCanvas.agent.activityPlanning", "正在规划下一步…"));
            await continueOnlineToolLoopAfterResults(session.id, assistantId, previousMessages, toolCalls, results, pendingContext?.step || Number(detail.step) || 1);
        } catch (error) {
            addOnlineLog(canvasT("videoCanvas.agent.continueRunFailed", "工具续跑失败"), error instanceof Error ? error.message : error);
            appendMessage(session.id, { id: nanoid(), role: "error", title: canvasT("videoCanvas.agent.opFailed", "操作失败"), text: error instanceof Error ? error.message : canvasT("videoCanvas.agent.opFailed", "操作失败") });
        } finally {
            setAgentActivity(null);
            setIsRunning(false);
        }
    };

    const rebuildPendingToolMessages = async (session: CanvasAssistantSession) => {
        const userMessage = lastUserMessage(session);
        if (!userMessage) return [];
        const history = pendingToolMessageHistory(session).filter((message) => message.id !== userMessage.id).slice(-8);
        return buildToolAgentMessages(snapshotRef.current, history, userMessage, previousSnapshotRef.current);
    };

    const rejectOnlineTool = (messageId: string) => {
        const message = getMessageById(messageId);
        const detail = objectDetail(message?.detail);
        const session = getSessionByMessageId(messageId);
        addOnlineLog(canvasT("videoCanvas.agent.rejectTool", "拒绝工具"), { messageId });
        const pendingContext = resolvePendingOnlineToolContext(messageId, detail, session);
        dropPendingOnlineToolContext(messageId);
        if (session) upsertMessage(session.id, { id: messageId, role: "tool", title: canvasT("videoCanvas.agent.rejectedTitle", "已拒绝执行"), text: canvasT("videoCanvas.agent.rejectedText", "工具调用已取消"), detail: { ...objectDetail(session.messages.find((item) => item.id === messageId)?.detail), status: "rejected" } });
        if (!session || !pendingContext?.toolCalls.length || !pendingContext.messages.length || !pendingContext.assistantId) return;
        const rejected = pendingContext.toolCalls.map((call) => ({
            toolCallId: call.id,
            name: call.function.name,
            result: { ok: false as const, message: canvasT("videoCanvas.agent.rejectedText", "工具调用已取消") },
        }));
        void continueOnlineToolLoopAfterResults(session.id, pendingContext.assistantId, pendingContext.messages, pendingContext.toolCalls, rejected, pendingContext.step);
    };

    return { isRunning, agentActivity, onlineLogs, addOnlineLog, clearOnlineLogs: () => setOnlineLogs([]), sendMessage, approveOnlineTool, rejectOnlineTool, executeOps, setIsRunning };
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

async function buildToolAgentMessages(snapshot: CanvasAgentSnapshot, history: CanvasAssistantMessage[], userMessage: CanvasAssistantMessage, previous?: CanvasAgentSnapshot | null): Promise<ResponseInputMessage[]> {
    const refs = userMessage.references || [];
    const mentioned = resolveCanvasAgentNodeIds(snapshot, parseCanvasAgentMentionTokens(userMessage.text));
    const mentionLine = mentioned.ids.length ? `\n用户 @ 的节点：${mentioned.ids.join(", ")}` : "";
    const missingLine = mentioned.missing.length ? `\n未能解析的 @：${mentioned.missing.join(", ")}` : "";
    const launchLine = userMessage.modelContext?.trim() ? `\n\n${userMessage.modelContext.trim()}` : "";
    const observation = buildCanvasAgentObservation(snapshot, previous);
    return [
        { role: "system", content: canvasHarness.constitution() },
        { role: "system", content: observationPromptBlock(observation) },
        ...historyToModelMessages(history),
        {
            role: "user",
            content: [
                ...refs.flatMap((item) => (item.text ? [{ type: "text" as const, text: `选中节点 ${item.title}：${item.text}` }] : [])),
                { type: "text", text: `用户需求：${userMessage.text}${launchLine}\n\n当前画布：${JSON.stringify(compactSnapshot(snapshot))}${mentionLine}${missingLine}` },
                ...(await Promise.all(refs.filter((item) => item.dataUrl).map(async (item) => ({ type: "image_url" as const, image_url: { url: await imageToDataUrl(item) } })))),
            ],
        },
    ];
}

function historyToModelMessages(history: CanvasAssistantMessage[]): ResponseInputMessage[] {
    return history
        .filter((message) => message.role === "user" || message.role === "assistant" || message.role === "system" || message.role === "tool")
        .slice(-16)
        .map((message): ResponseInputMessage => {
            if (message.role === "tool") return { role: "assistant", content: `[工具 ${message.title || "canvas"}] ${message.text}` };
            return { role: message.role as "system" | "user" | "assistant", content: message.text };
        });
}

function refreshObservationMessages(messages: ResponseInputMessage[], snapshot: CanvasAgentSnapshot, previous?: CanvasAgentSnapshot | null): ResponseInputMessage[] {
    const observation = { role: "system" as const, content: observationPromptBlock(buildCanvasAgentObservation(snapshot, previous)) };
    const rest = messages.filter((message) => !(message.role === "system" && typeof message.content === "string" && message.content.startsWith("[画布观察]")));
    const systemIndex = rest.findIndex((message) => message.role === "system");
    if (systemIndex < 0) return [observation, ...rest];
    return [...rest.slice(0, systemIndex + 1), observation, ...rest.slice(systemIndex + 1)];
}

export function rememberSnapshotNodes(snapshot: CanvasAgentSnapshot, inspected: Set<string>) {
    snapshot.nodes.forEach((node) => inspected.add(node.id));
}

/** Nodes already on the canvas (and in the user-message snapshot) are known; do not block writes. */
export function rememberWriteTargets(name: string, args: Record<string, unknown>, snapshot: CanvasAgentSnapshot, inspected: Set<string>, created: Set<string>) {
    const ids = targetIdsForWrite(name, args, snapshot);
    ids.forEach((id) => {
        inspected.add(id);
        created.add(id);
    });
    return "";
}

function targetIdsForWrite(name: string, args: Record<string, unknown>, snapshot: CanvasAgentSnapshot) {
    const ids: string[] = [];
    if (name === "canvas_update_node" || name === "canvas_update_node_text" || name === "canvas_resize_node" || name === "canvas_create_variants" || name === "canvas_set_video_frames" || name === "canvas_run_generation") {
        const token = typeof args.id === "string" ? args.id : typeof args.nodeId === "string" ? args.nodeId : "";
        if (token) ids.push(token);
    }
    if (name === "canvas_delete_nodes" && Array.isArray(args.ids)) args.ids.forEach((id) => { if (typeof id === "string") ids.push(id); });
    if (name === "canvas_move_nodes" && Array.isArray(args.items)) args.items.forEach((item) => {
        if (item && typeof item === "object" && typeof (item as { id?: unknown }).id === "string") ids.push((item as { id: string }).id);
    });
    if (name === "canvas_apply" && Array.isArray(args.patches)) {
        args.patches.forEach((item) => {
            if (item && typeof item === "object" && typeof (item as { id?: unknown }).id === "string") ids.push((item as { id: string }).id);
        });
    }
    if ((name === "canvas_run" || name === "canvas_repair") && Array.isArray(args.nodeIds)) args.nodeIds.forEach((id) => { if (typeof id === "string") ids.push(id); });
    if (name === "canvas_apply_ops" && Array.isArray(args.ops)) {
        args.ops.forEach((op) => {
            if (!op || typeof op !== "object") return;
            const record = op as Record<string, unknown>;
            if (record.type === "update_node" && typeof record.id === "string") ids.push(record.id);
            if (record.type === "delete_node" && typeof record.id === "string") ids.push(record.id);
            if (record.type === "run_generation" && typeof record.nodeId === "string") ids.push(record.nodeId);
        });
    }
    return resolveCanvasAgentNodeIds(snapshot, ids).ids.filter((id) => snapshot.nodes.some((node) => node.id === id));
}

function defaultExtractTimes(snapshot: CanvasAgentSnapshot, nodeId: string) {
    const durationMs = Number(snapshot.nodes.find((node) => node.id === nodeId)?.metadata?.durationMs) || 0;
    if (!Number.isFinite(durationMs) || durationMs <= 0) return [0];
    return [0, Math.round(durationMs / 2), Math.max(0, durationMs - 1)];
}
