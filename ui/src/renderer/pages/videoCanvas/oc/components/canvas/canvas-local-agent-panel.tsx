import { memo, useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { App, Button, Tooltip } from "antd";
import { History, MessageSquareText, PlugZap, RotateCcw, Terminal } from "lucide-react";
import { motion } from "motion/react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { createClientId } from "@oc/lib/client-id";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import { useCanvasAgentStore, type AgentChatItem, type AgentPendingToolCall, type AgentThreadSummary } from "@oc/stores/canvas/use-canvas-agent-store";
import { canvasAgentPostconditionMessage, summarizeCanvasAgentOps, verifyCanvasAgentOps, type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { buildCanvasAgentPlan } from "@oc/lib/canvas/canvas-agent-plan";
import { buildCanvasAgentContext, findCanvasAgentNodes, getCanvasAgentConnection, getCanvasAgentGenerationTasks, getCanvasAgentNode, getCanvasAgentResources, validateCanvasAgentOps } from "@oc/lib/canvas/canvas-agent-context";
import { collectCanvasSkills } from "@oc/lib/canvas/canvas-skill-mentions";
import { isProjectAgentReadTool, isProjectAgentToolName, runProjectAgentTool } from "@oc/services/api/project-agent-tools";
import { AgentChatComposer, AgentChatMessage, AgentPanelTabs, AgentPendingToolCard, AgentWorkingMessage } from "./canvas-agent-chat-ui";
import { compactCanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-snapshot-compact";
import { requireString } from "./canvas-online-agent-tools";
import { AgentChatEmptyState } from "./canvas-agent-panel-chrome";
import { buildAgentComposerReferences } from "./canvas-assistant-panel-views";
import { AgentConnectView, AgentHistoryView, AgentLogView } from "./canvas-local-agent-views";
import { discoverAgentConfig, fetchAgentJson, normalizeHistoryMessages, postState, postToolResult, type AgentThreadResponse, type AgentThreadsResponse } from "./canvas-local-agent-api";
import { activityText, agentAttachmentToChatAttachment, agentMessageToChatMessage, eventTitle, formatAgentEvent, isConnectionErrorMessage, mergeAgentText, parseEventData, shouldLogAgentEvent, toolName, type AgentEventPayload } from "./canvas-local-agent-events";
import { attachmentPayloadBytes, clamp, createId, formatBytes, normalizeText, promptWithAttachments, readDataUrl } from "./canvas-local-agent-utils";

const PANEL_MOTION_SECONDS = 0.5;
const MAX_ATTACHMENTS = 6;
const MAX_ATTACHMENT_PAYLOAD_BYTES = 28 * 1024 * 1024;
const DEFAULT_AGENT_URL = "http://127.0.0.1:17371";
const LA = "videoCanvas.agent.local";

export const CanvasLocalAgentPanel = memo(function CanvasLocalAgentPanel({ snapshot, canUndoOps, undoOpsCount = 0, collapsed, embedded, headless, autoConnect, onApplyOps, onUndoOps }: { snapshot: CanvasAgentSnapshot; canUndoOps: boolean; undoOpsCount?: number; collapsed?: boolean; embedded?: boolean; headless?: boolean; autoConnect?: boolean; onApplyOps: (ops: CanvasAgentOp[]) => unknown; onUndoOps: () => CanvasAgentSnapshot | null }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const user = useUserStore((state) => state.user);
    const { message, modal } = App.useApp();
    const [searchParams] = useSearchParams();
    const { width, url, token, connected, enabled, prompt, attachments, sending, waiting, messages, eventLogs, threads, activeThreadId, workspacePath, loadingThreads, activeTab, confirmTools, activity, connectError, pendingTool, setAgentState, addMessage: pushMessage, addEventLog: pushEventLog, clearEventLogs } = useCanvasAgentStore();
    const [resizing, setResizing] = useState(false);
    const listRef = useRef<HTMLDivElement>(null);
    const snapshotRef = useRef(snapshot);
    const confirmToolsRef = useRef(confirmTools);
    const pendingToolRef = useRef<AgentPendingToolCall | null>(null);
    const onApplyOpsRef = useRef(onApplyOps);
    const autoConnectRef = useRef(false);
    const connectedRef = useRef(false);
    const errorLoggedRef = useRef(false);
    const attachmentUrlsRef = useRef(new Set<string>());
    const clientIdRef = useRef(createClientId());
    const endpoint = useMemo(() => url.trim().replace(/\/$/, ""), [url]);
    // 映射结果按 messages 数组身份缓存：配合 memo(AgentChatMessage)，
    // 流式更新只重渲实际变化的那条消息，而不是整张聊天列表。
    const chatMessages = useMemo(() => messages.map(agentMessageToChatMessage), [messages]);
    const urlAgentAutoConnect = searchParams.has("agentUrl") && searchParams.has("agentToken");
    const loadThreads = useCallback(async () => {
        const projectId = snapshotRef.current.projectId;
        if ((!connectedRef.current && !useCanvasAgentStore.getState().connected) || !projectId) return;
        setAgentState({ loadingThreads: true });
        try {
            const data = await fetchAgentJson<AgentThreadsResponse>(endpoint, token, `/agent/codex/threads?canvasId=${encodeURIComponent(projectId)}`);
            const current = useCanvasAgentStore.getState();
            setAgentState({
                threads: data.data || [],
                workspacePath: data.workspace?.workspacePath || current.workspacePath,
                activeThreadId: data.workspace?.activeThreadId || current.activeThreadId,
            });
            const nextThreadId = data.workspace?.activeThreadId || current.activeThreadId;
            if (nextThreadId && !current.messages.length) {
                const thread = await fetchAgentJson<AgentThreadResponse>(endpoint, token, `/agent/codex/threads/${encodeURIComponent(nextThreadId)}?canvasId=${encodeURIComponent(projectId)}`);
                setAgentState({ messages: normalizeHistoryMessages(thread.messages || []) });
            }
        } catch (error) {
            addEventLog(canvasT(`${LA}.readHistoryFailed`, "读取历史失败"), error);
        } finally {
            setAgentState({ loadingThreads: false });
        }
    }, [endpoint, setAgentState, token]);

    useEffect(() => {
        snapshotRef.current = snapshot;
    }, [snapshot]);
    useEffect(() => {
        confirmToolsRef.current = confirmTools;
    }, [confirmTools]);
    useEffect(() => {
        pendingToolRef.current = pendingTool;
    }, [pendingTool]);
    useEffect(() => {
        onApplyOpsRef.current = onApplyOps;
    }, [onApplyOps]);
    useEffect(() => {
        if (activeTab !== "chat") return;
        const frame = requestAnimationFrame(() => listRef.current?.scrollTo({ top: listRef.current.scrollHeight }));
        return () => cancelAnimationFrame(frame);
    }, [activeTab, activeThreadId, messages, pendingTool, waiting]);
    useEffect(() => () => attachmentUrlsRef.current.forEach((url) => URL.revokeObjectURL(url)), []);

    useEffect(() => {
        if (!enabled || !token.trim()) return;
        localStorage.setItem("canvas-agent-url", endpoint);
        localStorage.setItem("canvas-agent-token", token);
        const clientId = clientIdRef.current;
        const source = new EventSource(`${endpoint}/events?token=${encodeURIComponent(token)}&clientId=${encodeURIComponent(clientId)}`);
        source.addEventListener("hello", () => {
            errorLoggedRef.current = false;
            connectedRef.current = true;
            setAgentState({ connected: true, activity: canvasT(`${LA}.activityConnected`, "已连接"), connectError: "", messages: useCanvasAgentStore.getState().messages.filter((item) => !isConnectionErrorMessage(item)) });
            if (!headless) message.success(canvasT(`${LA}.connectedToast`, "本地 Agent 已连接"));
            void postState(endpoint, token, clientId, snapshotRef.current);
        });
        source.addEventListener("tool_call", (event) => {
            const data = parseEventData<AgentPendingToolCall>(event);
            if (data) void handleToolCall(endpoint, token, data);
        });
        source.addEventListener("agent_event", (event) => {
            const data = parseEventData<AgentEventPayload>(event);
            if (data) handleAgentEvent(data);
        });
        source.addEventListener("agent_log", (event) => {
            const text = parseEventData<{ text?: unknown }>(event)?.text;
            addEventLog(canvasT(`${LA}.log`, "日志"), text, text);
        });
        source.addEventListener("agent_error", (event) => {
            const message = parseEventData<{ message?: unknown }>(event)?.message;
            setAgentState({ activity: canvasT(`${LA}.activityError`, "出错"), waiting: false });
            addMessage({ role: "error", title: canvasT(`${LA}.errorTitle`, "错误"), text: normalizeText(message) });
            addEventLog(canvasT(`${LA}.errorTitle`, "错误"), message, message);
        });
        source.addEventListener("agent_done", () => {
            setAgentState({ activity: canvasT(`${LA}.activityDone`, "完成"), waiting: false, sending: false });
            void loadThreads();
        });
        source.onerror = () => {
            const wasConnected = connectedRef.current;
            const text = wasConnected ? canvasT(`${LA}.errorConnectLost`, "本地 Agent 连接失败或已断开") : canvasT(`${LA}.errorConnectFailed`, "连接失败，请检查地址和 token");
            if (!errorLoggedRef.current || wasConnected) {
                addEventLog(wasConnected ? canvasT(`${LA}.connectLost`, "连接断开") : canvasT(`${LA}.connectFailed`, "连接失败"), { endpoint, error: text });
                if (!headless) message.error(text);
            }
            errorLoggedRef.current = true;
            connectedRef.current = false;
            clearAgentSession({ activity: wasConnected ? canvasT(`${LA}.activityDisconnected`, "连接断开") : canvasT(`${LA}.activityConnectFailed`, "连接失败"), connected: false, connectError: text });
            if (!wasConnected) {
                source.close();
                setAgentState({ enabled: false });
            }
        };
        return () => {
            source.close();
            connectedRef.current = false;
            setAgentState({ connected: false });
        };
    }, [enabled, endpoint, loadThreads, message, setAgentState, token]);

    useEffect(() => {
        if (connected) void loadThreads();
    }, [connected, loadThreads, snapshot.projectId]);

    useEffect(() => {
        if (!connected) return;
        const timer = setTimeout(() => void postState(endpoint, token, clientIdRef.current, snapshot), 300);
        return () => clearTimeout(timer);
    }, [connected, endpoint, snapshot, token]);

    const sendPrompt = async () => {
        const text = prompt.trim();
        const files = attachments;
        const requestPrompt = promptWithAttachments(text, files);
        if (!connected || !requestPrompt || sending || waiting) return;
        if (attachmentPayloadBytes(files) > MAX_ATTACHMENT_PAYLOAD_BYTES) {
            addMessage({ role: "error", title: canvasT(`${LA}.errorImageTooLarge`, "图片过大"), text: canvasT(`${LA}.errorImageTooLargeText`, "图片附件超过 30MB，请删减后再发送。") });
            return;
        }
        setAgentState({ activity: canvasT(`${LA}.activitySending`, "发送中"), sending: true, waiting: true });
        addMessage({ role: "user", text: text || canvasT(`${LA}.sentImages`, "发送了图片"), attachments: files });
        addEventLog(canvasT(`${LA}.userSend`, "用户发送"), { text, attachments: files.map(({ name, type, size }) => ({ name, type, size })) });
        try {
            const res = await fetch(`${endpoint}/agent/codex/turn?token=${encodeURIComponent(token)}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ prompt: requestPrompt, canvasId: snapshotRef.current.projectId, threadId: useCanvasAgentStore.getState().activeThreadId || undefined, attachments: files.map(({ name, type, dataUrl }) => ({ name, type, dataUrl })) }) });
            if (!res.ok) throw new Error(canvasT(`${LA}.errorRequestRejected`, "本地 Agent 拒绝了请求"));
            const data = (await res.json()) as { threadId?: string };
            if (data.threadId) setAgentState({ activeThreadId: data.threadId });
            addEventLog(canvasT(`${LA}.agentReceived`, "本地 Agent 已接收"), { status: res.status });
            files.forEach((item) => {
                URL.revokeObjectURL(item.url);
                attachmentUrlsRef.current.delete(item.url);
            });
            setAgentState({ prompt: "", attachments: [] });
        } catch (error) {
            setAgentState({ activity: canvasT(`${LA}.activitySendFailed`, "发送失败"), waiting: false });
            addMessage({ role: "error", title: canvasT(`${LA}.errorSendFailed`, "发送失败"), text: error instanceof Error ? error.message : canvasT(`${LA}.errorSendFailed`, "发送失败") });
            addEventLog(canvasT(`${LA}.errorSendFailed`, "发送失败"), error);
        } finally {
            setAgentState({ sending: false });
        }
    };

    const addAttachments = async (files: FileList | File[] | null) => {
        if (!files) return;
        const images = Array.from(files).filter((file) => file.type.startsWith("image/"));
        const prev = useCanvasAgentStore.getState().attachments;
        try {
            const next = await Promise.all(images.slice(0, Math.max(0, MAX_ATTACHMENTS - prev.length)).map(async (file) => {
                const dataUrl = await readDataUrl(file);
                const url = URL.createObjectURL(file);
                attachmentUrlsRef.current.add(url);
                return { id: createId(), name: file.name, type: file.type, size: file.size, url, dataUrl };
            }));
            const merged = [...prev, ...next];
            if (attachmentPayloadBytes(merged) > MAX_ATTACHMENT_PAYLOAD_BYTES) {
                next.forEach((item) => {
                    URL.revokeObjectURL(item.url);
                    attachmentUrlsRef.current.delete(item.url);
                });
                addMessage({ role: "error", title: canvasT(`${LA}.errorImageTooLarge`, "图片过大"), text: canvasT(`${LA}.errorImageMaxText`, "图片附件最多约 30MB。") });
                return;
            }
            if (next.length) setAgentState({ attachments: merged });
        } catch (error) {
            addMessage({ role: "error", title: canvasT(`${LA}.errorImageRead`, "图片读取失败"), text: error instanceof Error ? error.message : canvasT(`${LA}.errorImageRead`, "图片读取失败") });
        }
    };

    const removeAttachment = (id: string) => {
        const removed = attachments.find((item) => item.id === id);
        if (removed) {
            URL.revokeObjectURL(removed.url);
            attachmentUrlsRef.current.delete(removed.url);
        }
        setAgentState({ attachments: attachments.filter((item) => item.id !== id) });
    };

    const handleToolCall = async (endpoint: string, token: string, payload: AgentPendingToolCall) => {
        if (confirmToolsRef.current && (payload.name === "canvas_apply_ops" || (isProjectAgentToolName(payload.name) && !isProjectAgentReadTool(payload.name)))) {
            if (pendingToolRef.current) {
                await postToolResult(endpoint, token, clientIdRef.current, { requestId: payload.requestId, error: canvasT(`${LA}.pendingToolBusy`, "仍有待确认的画布工具调用") });
                return;
            }
            pendingToolRef.current = payload;
            setAgentState({ pendingTool: payload, activity: canvasT(`${LA}.activityWaitingConfirm`, "等待确认"), waiting: false });
            addEventLog(canvasT(`${LA}.waitingConfirm`, "等待确认"), payload, payload);
            return;
        }
        await runToolCall(endpoint, token, payload);
    };

	const runToolCall = async (endpoint: string, token: string, payload: AgentPendingToolCall) => {
		try {
			const input = (payload.input || {}) as Record<string, unknown>;
			const projectToolName = isProjectAgentToolName(payload.name) ? payload.name : null;
			setAgentState({ activity: payload.name === "canvas_apply_ops" ? canvasT(`${LA}.activityApplyingOps`, "执行画布操作") : projectToolName ? canvasT(`${LA}.activityProjectTool`, "执行项目工具") : canvasT(`${LA}.activityReadingCanvas`, "读取画布"), waiting: true });
			addEventLog(toolName(payload.name), payload, payload);
            if (payload.name === "canvas_apply_ops") {
                const currentSnapshot = snapshotRef.current;
                if (typeof input.expectedRevision === "number" && input.expectedRevision !== (currentSnapshot.revision ?? 0)) throw new Error(`画布 revision 已从 ${input.expectedRevision} 变为 ${currentSnapshot.revision ?? 0}，请重新读取 canvas_get_context 后再执行写操作`);
                const expectedStateHash = typeof input.expectedStateHash === "string" ? input.expectedStateHash : "";
                if (expectedStateHash && expectedStateHash !== buildCanvasAgentContext(currentSnapshot).stateHash) throw new Error("画布状态已变化，请重新读取 canvas_get_context 后再执行写操作。");
                const validation = validateCanvasAgentOps(currentSnapshot, (input.ops || []) as CanvasAgentOp[]);
                if (!validation.ok) throw new Error(`画布操作校验失败：${validation.issues.filter((item) => item.severity === "error").map((item) => item.message).join("；")}`);
            }
            const result =
                payload.name === "canvas_apply_ops"
                    ? (() => {
                          const before = snapshotRef.current;
                          const next = onApplyOpsRef.current((input.ops || []) as CanvasAgentOp[]) as CanvasAgentSnapshot;
                          const verification = verifyCanvasAgentOps(before, next, (input.ops || []) as CanvasAgentOp[]);
                          return { ok: verification.ok, message: canvasAgentPostconditionMessage(verification), data: { verification, snapshot: next }, snapshot: next };
                      })()
                    : payload.name === "canvas_get_state" || payload.name === "canvas_export_snapshot"
                      ? snapshotRef.current
                      : payload.name === "canvas_get_context"
                        ? buildCanvasAgentContext(snapshotRef.current)
                        : payload.name === "canvas_find_nodes"
                          ? findCanvasAgentNodes(snapshotRef.current, input as Parameters<typeof findCanvasAgentNodes>[1])
                          : payload.name === "canvas_get_node"
                            ? getCanvasAgentNode(snapshotRef.current, { id: requireString(input.id, "id") })
                            : payload.name === "canvas_get_connection"
                              ? getCanvasAgentConnection(snapshotRef.current, { id: requireString(input.id, "id") })
                              : payload.name === "canvas_get_generation_tasks"
                                ? getCanvasAgentGenerationTasks(snapshotRef.current, input as Parameters<typeof getCanvasAgentGenerationTasks>[1])
                                : payload.name === "canvas_get_resources"
                                  ? getCanvasAgentResources(snapshotRef.current, input as Parameters<typeof getCanvasAgentResources>[1])
                                  : payload.name === "canvas_validate_ops"
                                    ? validateCanvasAgentOps(snapshotRef.current, (input.ops || []) as CanvasAgentOp[])
                                    : payload.name === "canvas_list_skills"
                                      ? collectCanvasSkills(snapshotRef.current.nodes).map((skill) => ({ skillId: skill.skill_id, name: skill.skill_name, description: skill.description, tag: skill.tag }))
                                      : payload.name === "canvas_get_skill"
                                        ? (() => {
                                              const skillId = typeof input.skillId === "string" ? input.skillId : "";
                                              const nameQuery = typeof input.name === "string" ? input.name.trim().toLocaleLowerCase() : "";
                                              const skill = collectCanvasSkills(snapshotRef.current.nodes).find((item) => item.skill_id === skillId || item.skill_name.toLocaleLowerCase() === nameQuery);
                                              if (!skill) throw new Error("未找到画布技能，请先调用 canvas_list_skills。");
                                              return { skillId: skill.skill_id, name: skill.skill_name, description: skill.description, instruction: skill.instruction || skill.description, version: skill.update_time };
                                          })()
                                        : payload.name === "canvas_get_selection"
                                          ? (() => {
                                                const ids = new Set(snapshotRef.current.selectedNodeIds || []);
                                                return { nodes: snapshotRef.current.nodes.filter((node) => ids.has(node.id)) };
                                            })()
                                          : projectToolName
                                            ? await runProjectAgentTool(projectToolName, input, snapshotRef.current.domainProjectId)
                                            : snapshotRef.current;
            await postToolResult(endpoint, token, clientIdRef.current, { requestId: payload.requestId, result });
            if (payload.name === "canvas_apply_ops") void postState(endpoint, token, clientIdRef.current, ((result as { snapshot?: CanvasAgentSnapshot }).snapshot || snapshotRef.current) as CanvasAgentSnapshot);
            setAgentState({ activity: canvasT(`${LA}.activityToolDone`, "工具完成"), waiting: true });
            const applyResult = payload.name === "canvas_apply_ops" ? (result as { snapshot?: CanvasAgentSnapshot; message?: string }) : null;
            const loggedResult = projectToolName || payload.name !== "canvas_apply_ops" ? result : compactCanvasAgentSnapshot((applyResult?.snapshot || result) as CanvasAgentSnapshot);
            addEventLog(canvasT(`${LA}.toolDone`, "{{name}}完成", { name: toolName(payload.name) }), loggedResult, loggedResult);
            addMessage({ role: "tool", title: canvasT(`${LA}.toolDone`, "{{name}}完成", { name: toolName(payload.name) }), text: payload.name === "canvas_apply_ops" ? (applyResult?.message || summarizeCanvasAgentOps((input.ops || []) as CanvasAgentOp[]) || canvasT(`${LA}.canvasOp`, "画布操作")) : canvasT(`${LA}.completed`, "已完成"), detail: { requestId: payload.requestId, name: payload.name, input, result: loggedResult } });
        } catch (error) {
            const message = error instanceof Error ? error.message : canvasT(`${LA}.errorCanvasOpFailed`, "画布操作失败");
            setAgentState({ activity: canvasT(`${LA}.activityToolFailed`, "工具失败"), waiting: false });
            addMessage({ role: "tool", title: canvasT(`${LA}.errorToolFailed`, "工具失败"), text: message, detail: payload });
            await postToolResult(endpoint, token, clientIdRef.current, { requestId: payload.requestId, error: message });
        }
    };

    const rejectPendingTool = async () => {
        if (!pendingTool) return;
        await postToolResult(endpoint, token, clientIdRef.current, { requestId: pendingTool.requestId, error: canvasT(`${LA}.userCancelledTool`, "用户取消了画布工具调用") });
        setAgentState({ activity: canvasT(`${LA}.activityCancelled`, "已取消"), waiting: false });
        addMessage({ role: "tool", title: canvasT(`${LA}.rejectExec`, "拒绝执行"), text: toolName(pendingTool.name), detail: { requestId: pendingTool.requestId, name: pendingTool.name, input: pendingTool.input } });
        pendingToolRef.current = null;
        setAgentState({ pendingTool: null });
    };

    const approvePendingTool = async () => {
        if (!pendingTool) return;
        const tool = pendingTool;
        pendingToolRef.current = null;
        setAgentState({ pendingTool: null });
        await runToolCall(endpoint, token, tool);
    };

    const undoLastTool = () => {
        const restored = onUndoOps();
        if (!restored) return;
        setAgentState({ activity: canvasT(`${LA}.activityUndone`, "已撤销") });
        addMessage({ role: "tool", title: canvasT(`${LA}.undoneBatch`, "已撤销 Agent 批次"), text: canvasT(`${LA}.undoneText`, "已恢复到本次写回前的画布状态"), detail: compactCanvasAgentSnapshot(restored) });
        if (connected) void postState(endpoint, token, clientIdRef.current, restored);
    };

    const toggleAgentConnection = async () => {
        if (enabled) {
            clearAgentSession({ enabled: false, connected: false, activity: canvasT(`${LA}.activityOffline`, "离线"), connectError: "" });
            return;
        }
        const urlToken = searchParams.get("agentToken") || "";
        const urlEndpoint = searchParams.get("agentUrl") || "";
        const discovered = urlToken ? null : await discoverAgentConfig(endpoint || DEFAULT_AGENT_URL);
        const nextEndpoint = (urlEndpoint || discovered?.url || endpoint || DEFAULT_AGENT_URL).trim().replace(/\/$/, "");
        const nextToken = (urlToken || token.trim() || discovered?.token || "").trim();
        if (!nextEndpoint) {
            const text = canvasT(`${LA}.errorNeedUrl`, "请填写本地 Agent 地址");
            setAgentState({ connectError: text });
            if (!headless) message.warning(text);
            return;
        }
        if (!nextToken) {
            const text = canvasT(`${LA}.errorNoAgent`, "没有发现本地 Agent，请先在 Codex 使用插件或手动启动 Canvas Agent");
            setAgentState({ connectError: text });
            if (!headless) message.warning(text);
            return;
        }
        try {
            const parsed = new URL(nextEndpoint);
            if (parsed.protocol !== "http:" && parsed.protocol !== "https:") throw new Error("invalid protocol");
        } catch {
            const text = canvasT(`${LA}.errorBadUrl`, "本地 Agent 地址格式不正确");
            setAgentState({ connectError: text });
            if (!headless) message.warning(text);
            return;
        }
        errorLoggedRef.current = false;
        setAgentState({ url: nextEndpoint, token: nextToken, enabled: true, connected: false, activity: canvasT(`${LA}.activityConnecting`, "连接中"), connectError: "", activeTab: "setup" });
    };

    useEffect(() => {
        if (urlAgentAutoConnect && confirmTools) setAgentState({ confirmTools: false });
    }, [confirmTools, setAgentState, urlAgentAutoConnect]);

    useEffect(() => {
        if (!autoConnect || autoConnectRef.current || enabled || connected) return;
        autoConnectRef.current = true;
        void toggleAgentConnection();
    }, [autoConnect, connected, enabled]);

    function clearAgentSession(patch: Parameters<typeof setAgentState>[0] = {}) {
        setAgentState({
            messages: [],
            threads: [],
            activeThreadId: "",
            workspacePath: "",
            loadingThreads: false,
            waiting: false,
            sending: false,
            pendingTool: null,
            ...patch,
        });
        pendingToolRef.current = null;
    }

    const startNewThread = async () => {
        const projectId = snapshotRef.current.projectId;
        if (!connected || !projectId) return;
        setAgentState({ loadingThreads: true });
        try {
            const data = await fetchAgentJson<AgentThreadResponse>(endpoint, token, "/agent/codex/threads/new", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ canvasId: projectId }) });
            setAgentState({ activeThreadId: data.thread?.id || data.workspace?.activeThreadId || "", messages: [], activeTab: "chat", activity: canvasT(`${LA}.activityNewChat`, "新对话") });
            await loadThreads();
        } catch (error) {
            addEventLog(canvasT(`${LA}.newThreadFailed`, "新建对话失败"), error);
            message.error(error instanceof Error ? error.message : canvasT(`${LA}.newThreadFailed`, "新建对话失败"));
        } finally {
            setAgentState({ loadingThreads: false });
        }
    };

    const resumeThread = async (threadId: string) => {
        const projectId = snapshotRef.current.projectId;
        if (!connected || !projectId || !threadId) return;
        setAgentState({ loadingThreads: true });
        try {
            const data = await fetchAgentJson<AgentThreadResponse>(endpoint, token, `/agent/codex/threads/${encodeURIComponent(threadId)}/resume`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ canvasId: projectId }) });
            setAgentState({ activeThreadId: data.thread?.id || threadId, messages: normalizeHistoryMessages(data.messages || []), activeTab: "chat", activity: canvasT(`${LA}.activityResumed`, "已恢复会话") });
            await loadThreads();
        } catch (error) {
            addEventLog(canvasT(`${LA}.resumeFailed`, "恢复对话失败"), error);
            message.error(error instanceof Error ? error.message : canvasT(`${LA}.resumeFailed`, "恢复对话失败"));
        } finally {
            setAgentState({ loadingThreads: false });
        }
    };

    const deleteThread = async (threadId: string) => {
        const projectId = snapshotRef.current.projectId;
        if (!connected || !projectId || !threadId) return;
        setAgentState({ loadingThreads: true });
        try {
            await fetchAgentJson(endpoint, token, `/agent/codex/threads/${encodeURIComponent(threadId)}/delete`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ canvasId: projectId }) });
            const current = useCanvasAgentStore.getState();
            setAgentState({
                threads: current.threads.filter((thread) => thread.id !== threadId),
                activeThreadId: current.activeThreadId === threadId ? "" : current.activeThreadId,
                messages: current.activeThreadId === threadId ? [] : current.messages,
            });
            message.success(canvasT(`${LA}.deletedToast`, "记录已删除"));
        } catch (error) {
            addEventLog(canvasT(`${LA}.deleteFailed`, "删除对话失败"), error);
            message.error(error instanceof Error ? error.message : canvasT(`${LA}.deleteFailed`, "删除对话失败"));
        } finally {
            setAgentState({ loadingThreads: false });
        }
    };

    const confirmDeleteThread = (thread: AgentThreadSummary) => {
        const label = thread.name || thread.preview || canvasT(`${LA}.unnamedChat`, "未命名对话");
        const shortLabel = label.length > 48 ? `${label.slice(0, 48)}...` : label;
        modal.confirm({
            title: canvasT(`${LA}.deleteConfirmTitle`, "删除对话记录"),
            content: canvasT(`${LA}.deleteConfirmContent`, "确定删除「{{label}}」吗？", { label: shortLabel }),
            okText: canvasT("videoCanvas.agent.delete", "删除"),
            okType: "danger",
            cancelText: canvasT("videoCanvas.agent.cancel", "取消"),
            onOk: () => deleteThread(thread.id),
        });
    };

    const startResize = (event: ReactPointerEvent<HTMLDivElement>) => {
        event.preventDefault();
        const startX = event.clientX;
        const startWidth = width;
        let nextWidth = startWidth;
        const onMove = (moveEvent: PointerEvent) => {
            nextWidth = clamp(startWidth + startX - moveEvent.clientX, 360, 760);
            setAgentState({ width: nextWidth });
        };
        const onUp = () => {
            localStorage.setItem("canvas-agent-panel-width", String(nextWidth));
            window.removeEventListener("pointermove", onMove);
            window.removeEventListener("pointerup", onUp);
            setResizing(false);
        };
        setResizing(true);
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
    };

    const addMessage = (item: Omit<AgentChatItem, "id">) => {
        const text = normalizeText(item.text);
        if (!text && !item.attachments?.length) return;
        const next = { ...item, id: `${Date.now()}-${Math.random()}`, text };
        const currentMessages = useCanvasAgentStore.getState().messages;
        if (next.streamId) {
            const index = currentMessages.findIndex((message) => message.streamId === next.streamId);
            if (index >= 0) {
                setAgentState({ messages: currentMessages.map((message, i) => i === index ? { ...message, ...next, id: message.id, text: next.text || message.text } : message) });
                return;
            }
        }
        const last = currentMessages.at(-1);
        if (last?.role === "assistant" && next.role === "assistant" && last.title === next.title) {
            const merged = mergeAgentText(last.text, next.text);
            if (merged === last.text) return;
            setAgentState({ messages: [...useCanvasAgentStore.getState().messages.slice(0, -1), { ...last, text: merged, meta: next.meta || last.meta }] });
            return;
        }
        pushMessage(next);
    };

    const addEventLog = (title: string, text: unknown, raw?: unknown) => {
        pushEventLog({ id: `${Date.now()}-${Math.random()}`, time: new Date().toLocaleTimeString(), title, text: normalizeText(text) || title, raw });
    };

    const handleAgentEvent = (event: AgentEventPayload) => {
        if (shouldLogAgentEvent(event)) addEventLog(eventTitle(event), event, event);
        if (event.type === "thread.started" && event.thread_id) setAgentState({ activeThreadId: event.thread_id });
        const nextActivity = activityText(event);
        if (nextActivity) setAgentState({ activity: nextActivity });
        if (event.type === "turn.started") setAgentState({ waiting: true });
        if (event.type === "turn.completed" || event.type === "turn.failed" || event.type === "error") setAgentState({ waiting: false, sending: false });
        const item = formatAgentEvent(event);
        if (item) {
            if (item.role === "error") setAgentState({ waiting: false, sending: false });
            addMessage(item);
        }
    };

    const content = (
        <>
            <AgentPanelTabs
                value={activeTab}
                theme={theme}
                items={[
                    { value: "setup", label: canvasT(`${LA}.tabConnect`, "连接"), icon: <PlugZap className="size-3.5" /> },
                    { value: "chat", label: canvasT("videoCanvas.agent.tabChat", "对话"), icon: <MessageSquareText className="size-3.5" /> },
                    { value: "history", label: canvasT("videoCanvas.agent.tabHistory", "历史"), icon: <History className="size-3.5" />, count: threads.length },
                    { value: "log", label: canvasT(`${LA}.tabLog`, "日志"), icon: <Terminal className="size-3.5" />, count: eventLogs.length },
                ]}
                onChange={(activeTab) => {
                    setAgentState({ activeTab });
                    if (activeTab === "history") void loadThreads();
                }}
                right={
                    <>
                        <Tooltip title={undoOpsCount ? canvasT("videoCanvas.agent.undoBatch", "撤销最近一批 Agent 写回，可撤销 {{count}} 批", { count: undoOpsCount }) : canvasT("videoCanvas.agent.undoEmpty", "没有可撤销的 Agent 写回")}>
                            <Button size="small" type="text" className="!h-8 !w-8 !min-w-8" disabled={!canUndoOps} icon={<RotateCcw className="size-3.5" />} onClick={undoLastTool} aria-label={canvasT("videoCanvas.agent.undoAria", "撤销最近一批 Agent 写回")} />
                        </Tooltip>
                    </>
                }
            />

            {activeTab === "setup" ? (
                <AgentConnectView
                    theme={theme}
                    url={url}
                    token={token}
                    enabled={enabled}
                    connected={connected}
                    activity={activity}
                    connectError={connectError}
                    onUrlChange={(url) => setAgentState({ url, connectError: "" })}
                    onTokenChange={(token) => setAgentState({ token, connectError: "" })}
                    onToggleEnabled={toggleAgentConnection}
                />
            ) : activeTab === "history" ? (
                <AgentHistoryView
                    theme={theme}
                    threads={threads}
                    activeThreadId={activeThreadId}
                    workspacePath={workspacePath}
                    loading={loadingThreads}
                    connected={connected}
                    onRefresh={() => void loadThreads()}
                    onNewThread={() => void startNewThread()}
                    onResumeThread={(threadId) => void resumeThread(threadId)}
                    onDeleteThread={confirmDeleteThread}
                />
            ) : activeTab === "log" ? (
                <AgentLogView
                    logs={eventLogs}
                    theme={theme}
                    context={{ endpoint, connected, enabled, activity, waiting, sending, messages: messages.length, pendingTool: pendingTool?.name }}
                    onClear={clearEventLogs}
                    onCopied={(text) => message.success(text)}
                    onCopyBlocked={(text) => message.warning(text)}
                />
            ) : (
                <>
                    <div ref={listRef} className="thin-scrollbar min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
                        {!messages.length && !pendingTool && !waiting ? <AgentChatEmptyState theme={theme} nodeCount={snapshot.nodes.length} onSelect={(prompt) => setAgentState({ prompt })} /> : null}
                        {chatMessages.map((item) => (
                            <AgentChatMessage key={item.id} item={item} theme={theme} user={user} />
                        ))}
                        {pendingTool ? <AgentPendingToolCard summary={summarizeCanvasAgentOps(pendingTool.input?.ops || []) || toolName(pendingTool.name)} detail={{ requestId: pendingTool.requestId, name: pendingTool.name, input: pendingTool.input, impact: buildCanvasAgentPlan(pendingTool.input?.ops || [], snapshot) }} theme={theme} onReject={rejectPendingTool} onApprove={approvePendingTool} /> : null}
                        {waiting && !pendingTool ? <AgentWorkingMessage theme={theme} /> : null}
                    </div>
                    <AgentChatComposer
                        prompt={prompt}
                        attachments={attachments.map(agentAttachmentToChatAttachment)}
                        disabled={!connected}
                        sending={sending || waiting}
                        placeholder={canvasT(`${LA}.placeholder`, "询问本机编码 Agent，或让它操作画布")}
                        theme={theme}
                        mentionReferences={buildAgentComposerReferences(snapshot.nodes)}
                        onPromptChange={(prompt) => setAgentState({ prompt })}
                        onSubmit={sendPrompt}
                        onAddFiles={addAttachments}
                        onRemoveAttachment={removeAttachment}
                        left={attachments.length ? <span className="text-[var(--fs-label)]" style={{ color: theme.node.muted }}>{formatBytes(attachmentPayloadBytes(attachments))} / 30MB</span> : null}
                    />
                </>
            )}
        </>
    );

    if (headless) return null;
    if (embedded) return content;

    return (
        <motion.div
            className="relative z-[var(--z-panel-floating)] flex h-full shrink-0"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: collapsed ? 0 : width + 1, opacity: collapsed ? 0 : 1 }}
            transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: [0.22, 1, 0.36, 1] }}
            style={{ overflow: "clip", pointerEvents: collapsed ? "none" : undefined }}
        >
            <motion.aside
                className="relative flex h-full shrink-0 flex-col border-l"
                initial={{ x: 48 }}
                animate={{ x: collapsed ? 28 : 0 }}
                transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: [0.22, 1, 0.36, 1] }}
                style={{ width, background: theme.node.panel, borderColor: theme.node.stroke, color: theme.node.text }}
            >
                <div className="absolute left-0 top-0 h-full w-1 cursor-col-resize transition hover:bg-current/20" onPointerDown={startResize} />
                {content}
            </motion.aside>
        </motion.div>
    );
});
