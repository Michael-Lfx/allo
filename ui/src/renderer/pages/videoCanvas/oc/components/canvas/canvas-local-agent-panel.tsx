import { memo, useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { App, Button, Input, Segmented, Tooltip } from "antd";
import copyToClipboard from "copy-to-clipboard";
import { Copy, FolderOpen, History, KeyRound, Link2, LoaderCircle, MessageSquareText, PlugZap, Plus, RefreshCw, RotateCcw, Terminal, Trash2 } from "lucide-react";
import { motion } from "motion/react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { createClientId } from "@oc/lib/client-id";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import { useCanvasAgentStore, type AgentAttachment, type AgentChatItem, type AgentEventLog, type AgentPanelTab, type AgentPendingToolCall, type AgentThreadSummary } from "@oc/stores/canvas/use-canvas-agent-store";
import { previewCanvasAgentOps, summarizeCanvasAgentOps, type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { isProjectAgentReadTool, isProjectAgentToolName, runProjectAgentTool } from "@oc/services/api/project-agent-tools";
import { AgentChatComposer, AgentChatMessage, AgentPanelTabs, AgentPendingToolCard, AgentWorkingMessage, type CanvasAgentChatAttachment } from "./canvas-agent-chat-ui";
import { AgentChatEmptyState } from "./canvas-agent-panel-chrome";

const PANEL_MOTION_SECONDS = 0.5;
const MAX_ATTACHMENTS = 6;
const MAX_ATTACHMENT_PAYLOAD_BYTES = 28 * 1024 * 1024;
const DEFAULT_AGENT_URL = "http://127.0.0.1:17371";
const LA = "videoCanvas.agent.local";

function getAgentConnectSteps() {
    return [
        { title: canvasT(`${LA}.stepInstallTitle`, "从仓库安装插件"), text: canvasT(`${LA}.stepInstallText`, "插件暂未上架公共目录，请按项目 README 添加仓库 marketplace；安装后新建 Codex 对话。") },
        { title: canvasT(`${LA}.stepOpenTitle`, "打开画布连接"), text: canvasT(`${LA}.stepOpenText`, "回到这里点击连接，网页会自动读取本机 Agent 配置。") },
        { title: canvasT(`${LA}.stepManualTitle`, "手动启动备用"), text: canvasT(`${LA}.stepManualText`, "如果自动发现失败，请按插件说明启动本地 Agent 后再回到这里连接。") },
    ];
}

type AgentEventPayload = {
    agent?: string;
    type?: string;
    thread_id?: string;
    item?: AgentEventItem;
    error?: { message?: string };
    message?: string;
    usage?: Record<string, unknown>;
};
type AgentEventItem = { id?: string; type?: string; text?: unknown; message?: unknown; server?: string; tool?: string; status?: string; arguments?: unknown; result?: unknown; error?: { message?: string } };

type AgentLogContext = { endpoint: string; connected: boolean; enabled: boolean; activity: string; waiting: boolean; sending: boolean; messages: number; pendingTool?: string };
type AgentWorkspace = { canvasId: string; workspacePath: string; activeThreadId?: string };
type AgentThreadsResponse = { ok?: boolean; workspace?: AgentWorkspace; data?: AgentThreadSummary[] };
type AgentThreadResponse = { ok?: boolean; workspace?: AgentWorkspace; thread?: AgentThreadSummary; messages?: AgentChatItem[] };
type AgentConfigResponse = { ok?: boolean; url?: string; token?: string; hasToken?: boolean };

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
			const result = payload.name === "canvas_apply_ops" ? onApplyOpsRef.current((input.ops || []) as CanvasAgentOp[]) : projectToolName ? await runProjectAgentTool(projectToolName, input, snapshotRef.current.domainProjectId) : snapshotRef.current;
            await postToolResult(endpoint, token, clientIdRef.current, { requestId: payload.requestId, result });
            if (payload.name === "canvas_apply_ops") void postState(endpoint, token, clientIdRef.current, result as CanvasAgentSnapshot);
            setAgentState({ activity: canvasT(`${LA}.activityToolDone`, "工具完成"), waiting: true });
            addEventLog(canvasT(`${LA}.toolDone`, "{{name}}完成", { name: toolName(payload.name) }), result, result);
            addMessage({ role: "tool", title: canvasT(`${LA}.toolDone`, "{{name}}完成", { name: toolName(payload.name) }), text: payload.name === "canvas_apply_ops" ? summarizeCanvasAgentOps((input.ops || []) as CanvasAgentOp[]) || canvasT(`${LA}.canvasOp`, "画布操作") : canvasT(`${LA}.completed`, "已完成"), detail: { requestId: payload.requestId, name: payload.name, input, result } });
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
        addMessage({ role: "tool", title: canvasT(`${LA}.undoneBatch`, "已撤销 Agent 批次"), text: canvasT(`${LA}.undoneText`, "已恢复到本次写回前的画布状态"), detail: restored });
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
                        {messages.map((item) => (
                            <AgentChatMessage key={item.id} item={agentMessageToChatMessage(item)} theme={theme} user={user} />
                        ))}
                        {pendingTool ? <AgentPendingToolCard summary={summarizeCanvasAgentOps(pendingTool.input?.ops || []) || toolName(pendingTool.name)} detail={{ requestId: pendingTool.requestId, name: pendingTool.name, input: pendingTool.input, impact: previewCanvasAgentOps(pendingTool.input?.ops || [], snapshot) }} theme={theme} onReject={rejectPendingTool} onApprove={approvePendingTool} /> : null}
                        {waiting && !pendingTool ? <AgentWorkingMessage theme={theme} /> : null}
                    </div>
                    <AgentChatComposer
                        prompt={prompt}
                        attachments={attachments.map(agentAttachmentToChatAttachment)}
                        disabled={!connected}
                        sending={sending || waiting}
                        placeholder={canvasT(`${LA}.placeholder`, "询问 Codex，或让它操作画布")}
                        theme={theme}
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

function AgentLogView({ logs, theme, context, onClear, onCopied, onCopyBlocked }: { logs: AgentEventLog[]; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; context: AgentLogContext; onClear: () => void; onCopied: (text: string) => void; onCopyBlocked: (text: string) => void }) {
    useTranslation();
    const [mode, setMode] = useState<"text" | "json">("text");
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const content = mode === "text" ? formatLogText(logs, context) : formatLogJson(logs, context);
    const lastError = [...logs].reverse().find((item) => /错误|失败|error|fail/i.test(`${item.title}\n${item.text}`));
    const copy = async (value = content, tip = canvasT(`${LA}.logCopied`, "日志已复制")) => {
        if (await copyToClipboard(value)) {
            onCopied(tip);
            return;
        }
        textareaRef.current?.focus();
        textareaRef.current?.select();
        onCopyBlocked(canvasT(`${LA}.logCopyManual`, "已选中日志，请手动复制"));
    };
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
            <div className="flex min-h-full flex-col gap-3">
                <div>
                    <div className="text-base font-semibold leading-6">{canvasT(`${LA}.logTitle`, "运行日志")}</div>
                </div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                    <Segmented size="small" value={mode} onChange={(value) => setMode(value as "text" | "json")} options={[{ label: canvasT("videoCanvas.agent.logDiagnose", "排查日志"), value: "text" }, { label: canvasT("videoCanvas.agent.logRawJson", "原始 JSON"), value: "json" }]} />
                    <div className="flex items-center gap-2">
                        <span className="text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.agent.logCount", "{{count}} 条", { count: logs.length })}</span>
                        <Button size="small" icon={<Copy className="size-3.5" />} onClick={() => void copy()}>{canvasT("videoCanvas.agent.copy", "复制")}</Button>
                        <Button size="small" disabled={!lastError} onClick={() => lastError && void copy(formatLogText([lastError], context), canvasT(`${LA}.recentErrorCopied`, "最近错误已复制"))}>{canvasT("videoCanvas.agent.recentError", "最近错误")}</Button>
                        <Button size="small" danger type="text" icon={<Trash2 className="size-3.5" />} disabled={!logs.length} onClick={onClear}>{canvasT("videoCanvas.agent.clear", "清空")}</Button>
                    </div>
                </div>
                <textarea
                    ref={textareaRef}
                    readOnly
                    value={content}
                    className="thin-scrollbar min-h-[360px] flex-1 resize-none rounded-md border-0 p-3 font-mono text-xs leading-5 outline-none"
                    style={{ background: theme.spatial.surface, color: theme.node.text }}
                    onFocus={(event) => event.currentTarget.select()}
                />
            </div>
        </div>
    );
}

function AgentConnectView({ theme, url, token, enabled, connected, activity, connectError, onUrlChange, onTokenChange, onToggleEnabled }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes]; url: string; token: string; enabled: boolean; connected: boolean; activity: string; connectError: string; onUrlChange: (value: string) => void; onTokenChange: (value: string) => void; onToggleEnabled: () => void }) {
    useTranslation();
    const statusText = connectError ? canvasT(`${LA}.statusFailed`, "连接失败") : connected ? activity : enabled ? canvasT(`${LA}.statusConnecting`, "连接中") : canvasT(`${LA}.statusDisconnected`, "未连接");
    const statusColor = connectError ? "#dc2626" : connected ? "#16a34a" : enabled ? "#d97706" : theme.node.muted;
    const steps = getAgentConnectSteps();
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
            <div className="space-y-4">
                <div>
                    <div className="text-base font-semibold leading-6">{canvasT(`${LA}.setupTitle`, "连接本地 Agent")}</div>
                    <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>
                        {canvasT(`${LA}.setupHint`, "安装仓库自带的 Codex 插件后，画布会优先自动连接本机 Agent。")}
                    </div>
                </div>
                <div className="space-y-2">
                    {steps.map((step) => (
                        <div key={step.title} className="rounded-lg px-3 py-2.5">
                            <div className="text-sm font-medium leading-5">{step.title}</div>
                            <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>{step.text}</div>
                        </div>
                    ))}
                </div>
                <div className="rounded-md p-3" style={{ background: theme.spatial.surface }}>
                    <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                            <div className="flex min-w-0 items-center gap-2">
                                <span className="shrink-0 text-sm font-medium leading-5">{canvasT(`${LA}.webConnect`, "网页连接")}</span>
                                <span className="inline-flex min-w-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[var(--fs-label)] leading-4" style={{ background: theme.node.fill, color: statusColor }}>
                                    <span className="size-1.5 shrink-0 rounded-full" style={{ background: statusColor }} />
                                    <span className="truncate">{statusText}</span>
                                </span>
                            </div>
                            <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>
                                {canvasT(`${LA}.webConnectHint`, "默认自动读取 Local URL 和 Connect token，失败时再手动填写。")}
                            </div>
                        </div>
                        <Button className="!h-8 !px-3" type={enabled ? "default" : "primary"} icon={<PlugZap className="size-4" />} onClick={onToggleEnabled}>
                            {enabled ? canvasT(`${LA}.disconnect`, "断开") : canvasT(`${LA}.connect`, "连接")}
                        </Button>
                    </div>
                    <div className="mt-3 grid gap-2.5">
                        <label className="grid gap-1.5">
                            <span className="flex items-center gap-1.5 text-xs font-medium" style={{ color: theme.node.muted }}>
                                <Link2 className="size-3.5" />
                                {canvasT(`${LA}.localUrl`, "本地地址")}
                                <span className="font-normal opacity-70">Local URL</span>
                            </span>
                            <Input size="large" prefix={<Link2 className="mr-1 size-4" style={{ color: theme.node.faint }} />} value={url} onChange={(event) => onUrlChange(event.target.value)} placeholder={canvasT(`${LA}.urlPlaceholder`, "例如 http://127.0.0.1:17371")} />
                        </label>
                        <label className="grid gap-1.5">
                            <span className="flex items-center gap-1.5 text-xs font-medium" style={{ color: theme.node.muted }}>
                                <KeyRound className="size-3.5" />
                                {canvasT(`${LA}.connectToken`, "连接 Token")}
                                <span className="font-normal opacity-70">Connect token</span>
                            </span>
                            <Input.Password size="large" prefix={<KeyRound className="mr-1 size-4" style={{ color: theme.node.faint }} />} value={token} onChange={(event) => onTokenChange(event.target.value)} placeholder={canvasT(`${LA}.tokenPlaceholder`, "自动发现，或手动填入 Connect token")} />
                        </label>
                        {connectError ? (
                            <div className="rounded-md px-2.5 py-2 text-xs leading-5" style={{ background: "rgba(220,38,38,.08)", color: "#dc2626" }}>
                                {connectError}
                            </div>
                        ) : null}
                    </div>
                </div>
            </div>
        </div>
    );
}

function AgentHistoryView({ theme, threads, activeThreadId, workspacePath, loading, connected, onRefresh, onNewThread, onResumeThread, onDeleteThread }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes]; threads: AgentThreadSummary[]; activeThreadId: string; workspacePath: string; loading: boolean; connected: boolean; onRefresh: () => void; onNewThread: () => void; onResumeThread: (threadId: string) => void; onDeleteThread: (thread: AgentThreadSummary) => void }) {
    useTranslation();
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-3">
            <div className="space-y-3">
                <div className="flex min-w-0 items-center gap-2 text-xs" style={{ color: theme.node.muted }}>
                    <FolderOpen className="size-3.5 shrink-0" />
                    <span className="shrink-0">{canvasT(`${LA}.workspace`, "工作空间")}</span>
                    <span className="min-w-0 truncate" title={workspacePath}>{workspacePath || canvasT(`${LA}.defaultWorkspace`, "默认画布目录")}</span>
                </div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="text-sm" style={{ color: theme.node.muted }}>
                        {threads.length ? canvasT("videoCanvas.agent.historyCount", "{{count}} 条历史", { count: threads.length }) : connected ? canvasT("videoCanvas.agent.noHistory", "暂无历史") : canvasT(`${LA}.notConnected`, "未连接")}
                    </div>
                    <div className="flex items-center gap-2">
                        <Button size="small" icon={<RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />} disabled={!connected || loading} onClick={onRefresh}>
                            {canvasT(`${LA}.refresh`, "刷新")}
                        </Button>
                        <Button size="small" type="primary" icon={<Plus className="size-3.5" />} disabled={!connected || loading} onClick={onNewThread}>
                            {canvasT("videoCanvas.agent.newChat", "新对话")}
                        </Button>
                    </div>
                </div>
                <div className="space-y-2">
                    {threads.map((thread) => {
                        const active = thread.id === activeThreadId;
                        return (
                            <div key={thread.id} className="rounded-md px-2.5 py-2 transition-colors" style={{ background: active ? theme.accent.primarySoft : "transparent", color: theme.node.text }}>
                                <div className="flex items-center gap-2">
                                    <div className="min-w-0 flex-1">
                                        <div className="flex min-w-0 items-center gap-1.5">
                                            {active ? <span className="shrink-0 text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.text }}>{canvasT("videoCanvas.agent.current", "当前")}</span> : null}
                                            <div className="truncate text-sm font-medium leading-5">{thread.name || thread.preview || canvasT(`${LA}.unnamedChat`, "未命名对话")}</div>
                                        </div>
                                        <div className="truncate text-[var(--fs-label)] leading-4 opacity-65">{thread.preview || thread.id}</div>
                                    </div>
                                    <div className="flex shrink-0 items-center gap-1">
                                        <span className="text-[var(--fs-tiny)] opacity-55">{formatThreadTime(thread.updatedAt || thread.createdAt)}</span>
                                        <Button size="small" className="!h-6 !px-2" disabled={loading} onClick={() => onResumeThread(thread.id)}>
                                            {canvasT("videoCanvas.agent.enter", "进入")}
                                        </Button>
                                        <Tooltip title={canvasT("videoCanvas.agent.deleteRecord", "删除记录")}>
                                            <Button size="small" danger type="text" className="!h-6 !w-6 !min-w-6" disabled={loading} icon={<Trash2 className="size-3.5" />} onClick={() => onDeleteThread(thread)} />
                                        </Tooltip>
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                    {!threads.length ? (
                        <div className="px-3 py-8 text-center text-sm" style={{ color: theme.node.muted }}>
                            {connected ? canvasT(`${LA}.historyEmptyConnected`, "当前工作空间还没有对话记录") : canvasT(`${LA}.historyEmptyDisconnected`, "连接本地 Agent 后显示历史记录")}
                        </div>
                    ) : null}
                </div>
            </div>
        </div>
    );
}

async function postState(endpoint: string, token: string, clientId: string, snapshot: CanvasAgentSnapshot) {
    try {
        await fetch(`${endpoint}/canvas/state?token=${encodeURIComponent(token)}&clientId=${encodeURIComponent(clientId)}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(snapshot) });
    } catch {}
}

async function postToolResult(endpoint: string, token: string, clientId: string, body: { requestId: string; result?: unknown; error?: string }) {
    await fetch(`${endpoint}/canvas/result?token=${encodeURIComponent(token)}&clientId=${encodeURIComponent(clientId)}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
}

function agentMessageToChatMessage(item: AgentChatItem) {
    return { ...item, attachments: item.attachments?.map(agentAttachmentToChatAttachment) };
}

function agentAttachmentToChatAttachment(item: AgentAttachment): CanvasAgentChatAttachment {
    return { id: item.id, name: item.name, url: item.dataUrl || item.url };
}

function formatAgentEvent(event: AgentEventPayload): Omit<AgentChatItem, "id"> | null {
    const item = event.item;
    if (event.type === "item.completed" && item?.type === "error") return { role: "error", title: canvasT(`${LA}.errorTitle`, "错误"), text: normalizeText(item.message), detail: item };
    if ((event.type === "item.updated" || event.type === "item.completed") && item?.type === "agent_message") return { role: "assistant", title: canvasT(`${LA}.assistantTitle`, "Codex"), text: stringText(item.text), meta: usageText(event), streamId: item.id };
    if (event.type === "item.completed" && isMcpToolItem(item) && isReadTool(String(item?.tool || ""))) return { role: "tool", title: canvasT(`${LA}.toolDone`, "{{name}}完成", { name: toolName(String(item?.tool || "")) }), text: item?.error?.message || toolSummary(item), detail: toolDetail(item) };
    const text = eventText(event);
    if (text) return { role: "assistant", title: canvasT(`${LA}.assistantTitle`, "Codex"), text, meta: usageText(event) };
    return null;
}

function parseEventData<T>(event: Event) {
    try {
        return JSON.parse((event as MessageEvent).data) as T;
    } catch {
        return null;
    }
}

function formatLogText(logs: AgentEventLog[], context: AgentLogContext) {
    const connectionStatus = context.connected ? canvasT(`${LA}.diagOnline`, "在线") : context.enabled ? canvasT(`${LA}.diagConnecting`, "连接中") : canvasT(`${LA}.diagDisabled`, "未启用");
    const head = [
        canvasT(`${LA}.diagHeader`, "影策 Canvas Agent 诊断日志"),
        canvasT(`${LA}.diagCanvasAgent`, "Canvas Agent: {{endpoint}}", { endpoint: context.endpoint }),
        canvasT(`${LA}.diagConnection`, "连接: {{status}}", { status: connectionStatus }),
        canvasT(`${LA}.diagStatus`, "状态: {{activity}}", { activity: context.activity }),
        `waiting: ${context.waiting}`,
        `sending: ${context.sending}`,
        `messages: ${context.messages}`,
        `pendingTool: ${context.pendingTool ? toolName(context.pendingTool) : "none"}`,
        `logs: ${logs.length}`,
    ].join("\n");
    const body = logs.map((item, index) => {
        const detail = item.raw == null ? item.text : JSON.stringify(item.raw, null, 2);
        return [`#${index + 1} ${item.time} ${item.title}`, detail].filter(Boolean).join("\n");
    }).join("\n\n---\n\n");
    return [head, body || canvasT(`${LA}.diagNoEvents`, "暂无事件日志")].join("\n\n");
}

function formatLogJson(logs: AgentEventLog[], context: AgentLogContext) {
    return JSON.stringify({ context, logs: logs.map(({ time, title, text, raw }) => ({ time, title, text, raw })) }, null, 2);
}

function eventText(event: AgentEventPayload) {
    return event.type === "item.completed" && event.item?.type === "agent_message" ? stringText(event.item.text) : "";
}

function usageText(event: AgentEventPayload) {
    const usage = event.usage;
    if (!usage || typeof usage !== "object") return undefined;
    const total = numberField(usage, "total_tokens");
    const input = numberField(usage, "input_tokens");
    const output = numberField(usage, "output_tokens");
    if (total) return `${total} tok`;
    if (input || output) return `${input || 0}/${output || 0} tok`;
    return undefined;
}

function activityText(event: AgentEventPayload) {
    const name = event.type || "";
    if (name === "thread.started") return canvasT(`${LA}.activityThreadStarted`, "已创建会话");
    if (name === "turn.started") return canvasT(`${LA}.activityThinking`, "思考中");
    if (name === "turn.completed") return canvasT(`${LA}.activityDone`, "完成");
    if (name === "turn.failed" || name === "error") return canvasT(`${LA}.activityError`, "出错");
    if (name === "item.started") return isMcpToolItem(event.item) ? canvasT(`${LA}.activityCallingTool`, "调用{{name}}", { name: toolName(String(event.item?.tool || "")) }) : canvasT(`${LA}.activityStep`, "执行步骤");
    if (name === "item.completed") return isMcpToolItem(event.item) ? canvasT(`${LA}.activityToolDone`, "工具完成") : canvasT(`${LA}.activityMessageUpdated`, "更新消息");
    return "";
}

function eventTitle(event: AgentEventPayload) {
    const item = event.item;
    if (event.type === "thread.started") return canvasT(`${LA}.eventThreadStarted`, "已创建 Codex 会话");
    if (event.type === "turn.started") return canvasT(`${LA}.eventTurnStarted`, "开始处理");
    if (event.type === "turn.completed") return canvasT(`${LA}.eventTurnCompleted`, "本轮完成");
    if (event.type === "stream.summary") return canvasT(`${LA}.eventStreamSummary`, "流式摘要");
    if (event.type === "turn.failed" || event.type === "error") return canvasT(`${LA}.eventTurnFailed`, "本轮失败");
    if (event.type === "item.started" && isMcpToolItem(item)) return canvasT(`${LA}.eventCallTool`, "调用工具：{{name}}", { name: toolName(String(item?.tool || "")) });
    if (event.type === "item.completed" && isMcpToolItem(item)) return canvasT(`${LA}.eventToolDone`, "工具完成：{{name}}", { name: toolName(String(item?.tool || "")) });
    if (event.type === "item.completed" && item?.type === "agent_message") return canvasT(`${LA}.eventCodexReply`, "Codex 回复");
    return event.type || canvasT(`${LA}.eventCodex`, "Codex 事件");
}

function shouldLogAgentEvent(event: AgentEventPayload) {
    const itemType = event.item?.type || "";
    return !["item.updated"].includes(event.type || "") && !["reasoning"].includes(itemType) && !(event.type === "item.started" && itemType === "agent_message");
}

function isConnectionErrorMessage(item: AgentChatItem) {
    return item.role === "error" && /连接失败|无法连接本地 Agent|本地 Agent 连接失败|connection failed|unable to connect|local agent.*(failed|disconnected)/i.test(item.text);
}

const TOOL_NAME_KEYS: Record<string, string> = {
    canvas_apply_ops: "canvas_apply_ops",
    canvas_get_state: "canvas_get_state",
    canvas_get_selection: "canvas_get_selection",
    canvas_export_snapshot: "canvas_export_snapshot",
    canvas_create_node: "canvas_create_node",
    canvas_create_text_node: "canvas_create_text_node",
    canvas_create_text_nodes: "canvas_create_text_nodes",
    canvas_create_image_prompt_flow: "canvas_create_image_prompt_flow",
    canvas_create_generation_flow: "canvas_create_generation_flow",
    canvas_generate_text: "canvas_generate_text",
    canvas_generate_image: "canvas_generate_image",
    canvas_generate_video: "canvas_generate_video",
    canvas_generate_audio: "canvas_generate_audio",
    canvas_update_node: "canvas_update_node",
    canvas_update_node_text: "canvas_update_node_text",
    canvas_move_nodes: "canvas_move_nodes",
    canvas_resize_node: "canvas_resize_node",
    canvas_delete_nodes: "canvas_delete_nodes",
    canvas_connect_nodes: "canvas_connect_nodes",
    canvas_select_nodes: "canvas_select_nodes",
    canvas_set_viewport: "canvas_set_viewport",
    canvas_run_generation: "canvas_run_generation",
    project_get_context: "project_get_context",
    project_list_units: "project_list_units",
    project_extract_asset_candidates: "project_extract_asset_candidates",
    project_confirm_asset_candidate: "project_confirm_asset_candidate",
    project_create_or_update_shots: "project_create_or_update_shots",
    project_link_shot_asset: "project_link_shot_asset",
    project_start_workflow_step: "project_start_workflow_step",
    project_link_asset: "project_link_asset",
    project_upsert_asset_version: "project_upsert_asset_version",
    project_register_task_output: "project_register_task_output",
};

const TOOL_NAME_DEFAULTS: Record<string, string> = {
    canvas_apply_ops: "画布操作",
    canvas_get_state: "读取画布",
    canvas_get_selection: "读取选区",
    canvas_export_snapshot: "导出快照",
    canvas_create_node: "创建节点",
    canvas_create_text_node: "创建文本",
    canvas_create_text_nodes: "批量创建文本",
    canvas_create_image_prompt_flow: "创建生图流程",
    canvas_create_generation_flow: "创建生成流程",
    canvas_generate_text: "生成文本",
    canvas_generate_image: "生成图片",
    canvas_generate_video: "生成视频",
    canvas_generate_audio: "生成音频",
    canvas_update_node: "更新节点",
    canvas_update_node_text: "更新文本",
    canvas_move_nodes: "移动节点",
    canvas_resize_node: "调整节点尺寸",
    canvas_delete_nodes: "删除节点",
    canvas_connect_nodes: "连接节点",
    canvas_select_nodes: "选择节点",
    canvas_set_viewport: "调整视口",
    canvas_run_generation: "触发生成",
    project_get_context: "读取项目上下文",
    project_list_units: "读取项目章节",
    project_extract_asset_candidates: "登记资产候选",
    project_confirm_asset_candidate: "确认资产候选",
    project_create_or_update_shots: "保存项目镜头",
    project_link_shot_asset: "关联镜头素材",
    project_start_workflow_step: "启动流程步骤",
    project_link_asset: "引用项目资产",
    project_upsert_asset_version: "创建资产版本",
    project_register_task_output: "登记任务产物",
};

function toolName(name: string) {
    const key = TOOL_NAME_KEYS[name];
    if (!key) return name;
    return canvasT(`${LA}.tools.${key}`, TOOL_NAME_DEFAULTS[key] || name);
}

function isReadTool(name: string) {
    return name === "canvas_get_state" || name === "canvas_get_selection" || name === "canvas_export_snapshot" || isProjectAgentReadTool(name);
}

function isMcpToolItem(item?: AgentEventItem) {
    return item?.type === "mcp_tool_call";
}

function toolDetail(item?: AgentEventItem) {
    return { server: item?.server, tool: item?.tool, status: item?.status, arguments: item?.arguments, result: parseToolResult(item?.result), error: item?.error };
}

function toolSummary(item?: AgentEventItem) {
    const result = parseToolResult(item?.result);
    const nodeField = objectField(result, "nodes");
    const connectionField = objectField(result, "connections");
    const nodes = Array.isArray(nodeField) ? nodeField : [];
    const connections = Array.isArray(connectionField) ? connectionField : [];
    if (Array.isArray(nodeField) || Array.isArray(connectionField)) return canvasT(`${LA}.toolReadSummary`, "读取到 {{nodes}} 个节点，{{connections}} 条连线", { nodes: nodes.length, connections: connections.length });
    return canvasT(`${LA}.toolCallDone`, "工具调用完成");
}

function parseToolResult(result: unknown) {
    const content = objectField(result, "content");
    const text = Array.isArray(content) ? content.map((item) => objectField(item, "text")).filter((item): item is string => typeof item === "string").join("\n") : "";
    try {
        return text ? JSON.parse(text) : result;
    } catch {
        return text || result;
    }
}

function normalizeText(value: unknown) {
    if (typeof value === "string") return value.trim();
    if (value instanceof Error) return value.message;
    if (value == null) return "";
    return JSON.stringify(value, null, 2);
}

function stringText(value: unknown) {
    return typeof value === "string" ? value : "";
}

function objectField(value: unknown, key: string) {
    return value && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined;
}

function numberField(value: unknown, key: string) {
    const field = objectField(value, key);
    return typeof field === "number" ? field : 0;
}

function mergeAgentText(prev: string, next: string) {
    if (!next || prev === next || prev.endsWith(next)) return prev;
    if (next.startsWith(prev)) return next;
    for (let size = Math.min(prev.length, next.length); size > 0; size--) {
        if (prev.endsWith(next.slice(0, size))) return `${prev}${next.slice(size)}`;
    }
    const half = Math.floor(prev.length / 2);
    if (prev.length > 12 && next.length > 12 && prev.slice(half) === next.slice(0, prev.length - half)) return prev;
    return `${prev}${next}`;
}

function promptWithAttachments(text: string, attachments: AgentAttachment[]) {
    if (!attachments.length) return text;
    const names = attachments.map((item) => item.name).join("、");
    return [text, `用户上传了 ${attachments.length} 张图片附件：${names}。`].filter(Boolean).join("\n\n");
}

function attachmentPayloadBytes(attachments: AgentAttachment[]) {
    return attachments.reduce((total, item) => total + item.dataUrl.length, 0);
}

function formatBytes(bytes: number) {
    return bytes > 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)}MB` : `${Math.ceil(bytes / 1024)}KB`;
}

async function fetchAgentJson<T>(endpoint: string, token: string, path: string, init?: RequestInit) {
    const url = `${endpoint}${path}${path.includes("?") ? "&" : "?"}token=${encodeURIComponent(token)}`;
    const res = await fetch(url, init);
    const data = (await res.json().catch(() => ({}))) as T & { error?: string; msg?: string };
    if (!res.ok) throw new Error(data.error || data.msg || canvasT(`${LA}.errorRequestFailed`, "本地 Agent 请求失败"));
    return data;
}

async function discoverAgentConfig(endpoint: string) {
    try {
        const res = await fetch(`${endpoint}/config`);
        if (!res.ok) return null;
        const data = (await res.json()) as AgentConfigResponse;
        return data.ok ? data : null;
    } catch {
        return null;
    }
}

function normalizeHistoryMessages(messages: AgentChatItem[]) {
    return messages
        .map((item, index) => ({
            ...item,
            id: item.id || `history-${index}`,
            text: normalizeText(item.text),
        }))
        .filter((item) => item.text);
}

function formatThreadTime(value?: number) {
    if (!value) return "";
    return new Date(value * 1000).toLocaleString();
}

function createId() {
    return createClientId();
}

function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
}

function readDataUrl(file: File) {
    return new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result || ""));
        reader.onerror = () => reject(reader.error || new Error(canvasT(`${LA}.readImageFailed`, "读取图片失败")));
        reader.readAsDataURL(file);
    });
}
