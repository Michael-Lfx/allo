import { useTranslation } from "react-i18next";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { History, MessageSquareText, Plus, ScrollText, Settings2, X } from "lucide-react";
import { Button, Modal, Tooltip } from "antd";
import { motion } from "motion/react";

import { resolveModelRequestConfig, useConfigStore, useEffectiveConfig, type AiConfig } from "@oc/stores/use-config-store";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { nanoid } from "nanoid";
import { useAssetStore } from "@oc/stores/use-asset-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { cinematicAgentSessionOpsJson, createCinematicAgentSession, isAgentSessionPollingAbort, resumeCinematicAgentSession } from "@oc/lib/canvas/canvas-agent-session";
import { summarizeCanvasContext } from "@oc/lib/canvas/canvas-context-summary";
import { AgentChatComposer, AgentPanelTabs, type CanvasAgentMode } from "./canvas-agent-chat-ui";
import { compactCanvasAgentSnapshot as compactSnapshot } from "@oc/lib/canvas/canvas-agent-snapshot-compact";
import { AgentPanelChrome } from "./canvas-agent-panel-chrome";
import { CANVAS_AGENT_PANEL_MOTION_MS } from "./canvas-assistant-panel-motion";
import { CanvasLocalAgentPanel } from "./canvas-local-agent-panel";
import { type CanvasAssistantMessage, type CanvasAssistantPendingBackendSession, type CanvasAssistantSession, type CanvasNodeData } from "@oc/types/canvas";
import { useCanvasAgentStore } from "@oc/stores/canvas/use-canvas-agent-store";
import { summarizeCanvasAgentOps, type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { resolveStoryboardGenerationContext } from "@oc/lib/canvas/canvas-storyboard-context";
import { createSession, useCanvasOnlineAgentLoop } from "./canvas-online-agent-loop";
import { requireOps } from "./canvas-online-agent-tools";
import { AgentTextModelPicker, AssistantChatMessages, AssistantHistory, AssistantReferenceChip, OnlineAgentLogView, OnlineAgentSetupView, assistantImageReferenceLabel, buildAssistantReferences } from "./canvas-assistant-panel-views";

const PANEL_MOTION_SECONDS = CANVAS_AGENT_PANEL_MOTION_MS / 1000;
type OnlineAgentTab = "setup" | "chat" | "history" | "log";

type CanvasAssistantPanelProps = {
    nodes: CanvasNodeData[];
    selectedNodeIds: Set<string>;
    snapshot: CanvasAgentSnapshot;
    projectId: string;
    sessions: CanvasAssistantSession[];
    activeSessionId: string | null;
    onSelectNodeIds: (ids: Set<string>) => void;
    onSessionsChange: (sessions: CanvasAssistantSession[], activeSessionId: string | null) => void;
    onApplyOps: (ops?: CanvasAgentOp[]) => CanvasAgentSnapshot;
    canUndoOps: boolean;
    undoOpsCount: number;
    onUndoOps: () => CanvasAgentSnapshot | null;
    onPasteImage: (file: File) => void;
    agentMode: CanvasAgentMode;
    onAgentModeChange: (mode: CanvasAgentMode) => void;
    autoConnectLocal?: boolean;
    closing: boolean;
    onCollapse: () => void;
    cinematicEntry?: boolean;
    onCinematicEntryConsumed?: () => void;
    resizing?: boolean;
};

export function CanvasAssistantPanel({ nodes, selectedNodeIds, snapshot, projectId, sessions, activeSessionId, onSelectNodeIds, onSessionsChange, onApplyOps, canUndoOps, undoOpsCount, onUndoOps, onPasteImage, agentMode, onAgentModeChange, autoConnectLocal, closing, onCollapse, cinematicEntry = false, onCinematicEntryConsumed, resizing = false }: CanvasAssistantPanelProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const user = useUserStore((state) => state.user);
    const effectiveConfig = useEffectiveConfig();
    const cleanupImages = useAssetStore((state) => state.cleanupImages);
    const updateConfig = useConfigStore((state) => state.updateConfig);
    const confirmTools = useCanvasAgentStore((state) => state.confirmTools);
    const setAgentState = useCanvasAgentStore((state) => state.setAgentState);
    const [view, setView] = useState<OnlineAgentTab>("chat");
    const [prompt, setPrompt] = useState("");
    const [cinematicEntryActive, setCinematicEntryActive] = useState(cinematicEntry);
    const [deleteChatIds, setDeleteChatIds] = useState<string[]>([]);
    const [removedReferenceIds, setRemovedReferenceIds] = useState<Set<string>>(new Set());
    const [localSessions, setLocalSessions] = useState<CanvasAssistantSession[]>(() => (sessions.length ? sessions : [createSession()]));
    const [localActiveSessionId, setLocalActiveSessionId] = useState<string | null>(activeSessionId);
    const applyingExternalSessionsRef = useRef(false);
    const chatListRef = useRef<HTMLDivElement>(null);
    const snapshotRef = useRef(snapshot);
    const cinematicSessionControllersRef = useRef(new Map<string, AbortController>());

    const safeSessions = localSessions.length ? localSessions : [createSession()];
    const activeSession = useMemo(() => safeSessions.find((session) => session.id === localActiveSessionId) || safeSessions[0] || null, [localActiveSessionId, safeSessions]);
    const historySessions = safeSessions.filter((session) => session.messages.length > 0);
    const messages = activeSession?.messages || [];
    const hasMessages = messages.length > 0;
    const activeModel = effectiveConfig.textModel || effectiveConfig.model;
    const selectedNodeKey = useMemo(() => Array.from(selectedNodeIds).sort().join(","), [selectedNodeIds]);
    const allSelectedReferences = useMemo(() => buildAssistantReferences(nodes, selectedNodeIds), [nodes, selectedNodeIds]);
    const selectedReferences = useMemo(() => allSelectedReferences.filter((item) => !removedReferenceIds.has(item.id)), [allSelectedReferences, removedReferenceIds]);
    const contextSummary = useMemo(() => summarizeCanvasContext(nodes, selectedNodeIds), [nodes, selectedNodeIds]);
    const iconButtonStyle = { color: theme.node.muted };

    const updateSession = (sessionId: string, updater: (session: CanvasAssistantSession) => CanvasAssistantSession) => {
        setLocalSessions((prev) => prev.map((session) => (session.id === sessionId ? updater(session) : session)));
    };

    const appendMessage = (sessionId: string, message: CanvasAssistantMessage) => {
        updateSession(sessionId, (session) => ({
            ...session,
            title: session.messages.length ? session.title : message.text.slice(0, 18) || canvasT("videoCanvas.agent.newChatFallback", "新对话"),
            messages: [...session.messages, message],
            updatedAt: new Date().toISOString(),
        }));
    };

    const upsertMessage = (sessionId: string, message: CanvasAssistantMessage) => {
        updateSession(sessionId, (session) => {
            const exists = session.messages.some((item) => item.id === message.id);
            return {
                ...session,
                title: session.messages.length ? session.title : message.text.slice(0, 18) || canvasT("videoCanvas.agent.newChatFallback", "新对话"),
                messages: exists ? session.messages.map((item) => (item.id === message.id ? { ...item, ...message } : item)) : [...session.messages, message],
                updatedAt: new Date().toISOString(),
            };
        });
    };

    const { isRunning, onlineLogs, addOnlineLog, clearOnlineLogs, sendMessage, approveOnlineTool, rejectOnlineTool, executeOps, setIsRunning } = useCanvasOnlineAgentLoop({
        snapshotRef,
        config: effectiveConfig,
        confirmTools,
        selectedReferences,
        activeSession,
        onApplyOps,
        appendMessage,
        upsertMessage,
        getMessageById: (messageId) => safeSessions.flatMap((session) => session.messages).find((item) => item.id === messageId),
        getSessionByMessageId: (messageId) => safeSessions.find((session) => session.messages.some((item) => item.id === messageId)),
        activateSession: (session) => {
            setLocalSessions([session]);
            setLocalActiveSessionId(session.id);
        },
    });

    const agentBusy = isRunning || safeSessions.some((session) => session.pendingBackendSession?.status === "pending");

    useEffect(() => {
        if (!sessions.length) return;
        if (sessions === localSessions && activeSessionId === localActiveSessionId) return;
        applyingExternalSessionsRef.current = true;
        setLocalSessions(sessions);
        setLocalActiveSessionId(activeSessionId);
    }, [activeSessionId, sessions]);

    useEffect(() => {
        snapshotRef.current = snapshot;
    }, [snapshot]);

    useEffect(() => () => {
        // 收起面板或刷新页面时只停止前端查询，后台任务由下次挂载根据持久化 ID 继续接管。
        cinematicSessionControllersRef.current.forEach((controller) => controller.abort());
        cinematicSessionControllersRef.current.clear();
    }, []);

    useEffect(() => {
        if (applyingExternalSessionsRef.current) {
            applyingExternalSessionsRef.current = false;
            return;
        }
        if (sessions === localSessions && activeSessionId === localActiveSessionId) return;
        onSessionsChange(localSessions, localActiveSessionId);
    }, [activeSessionId, localActiveSessionId, localSessions, onSessionsChange, sessions]);

    useEffect(() => {
        if (agentMode !== "online" || view !== "chat") return;
        const frame = requestAnimationFrame(() => chatListRef.current?.scrollTo({ top: chatListRef.current.scrollHeight }));
        return () => cancelAnimationFrame(frame);
    }, [agentBusy, agentMode, localActiveSessionId, messages, view]);

    useEffect(() => {
        setRemovedReferenceIds(new Set());
    }, [selectedNodeKey]);

    // 稳定回调身份：配合 memo(AgentChatMessage) 避免任一状态变化重渲全部历史消息。
    const rejectOnlineToolRef = useRef(rejectOnlineTool);
    const approveOnlineToolRef = useRef(approveOnlineTool);
    useEffect(() => {
        rejectOnlineToolRef.current = rejectOnlineTool;
        approveOnlineToolRef.current = approveOnlineTool;
    });
    const stableRejectOnlineTool = useCallback((id: string) => rejectOnlineToolRef.current(id), []);
    const stableApproveOnlineTool = useCallback((id: string) => approveOnlineToolRef.current(id), []);

    const setPendingCinematicSession = (sessionId: string, backendSessionId: string) => {
        const startedAt = new Date().toISOString();
        const pending: CanvasAssistantPendingBackendSession = {
            id: backendSessionId,
            kind: "cinematic",
            messageId: cinematicSessionMessageId(backendSessionId),
            status: "pending",
            startedAt,
        };
        updateSession(sessionId, (session) => ({
            ...session,
            pendingBackendSession: pending,
            messages: upsertAssistantMessage(session.messages, {
                id: pending.messageId,
                role: "assistant",
                title: canvasT("videoCanvas.agent.cinePendingTitle", "影视项目生成中"),
                text: canvasT("videoCanvas.agent.cinePendingText", "后端影视 Agent 正在处理。即使页面刷新，也会在重新进入画布后继续等待结果。"),
                detail: { kind: "cinematic", backendSessionId, status: "pending", startedAt },
            }),
            updatedAt: startedAt,
        }));
    };

    const completeCinematicSession = (sessionId: string, backendSessionId: string, ops: CanvasAgentOp[], recovered = false) => {
        updateSession(sessionId, (session) => {
            const pending = session.pendingBackendSession;
            if (pending?.id !== backendSessionId) return session;
            const completedAt = new Date().toISOString();
            const summary = summarizeCanvasAgentOps(ops) || canvasT("videoCanvas.agent.cineWrittenSummary", "影视项目已写回当前画布。");
            return {
                ...session,
                pendingBackendSession: undefined,
                messages: upsertAssistantMessage(session.messages, {
                    id: pending.messageId,
                    role: "assistant",
                    title: recovered ? canvasT("videoCanvas.agent.cineRecoveredTitle", "影视项目已恢复并写回") : canvasT("videoCanvas.agent.cineWrittenTitle", "影视项目已写回"),
                    text: recovered ? canvasT("videoCanvas.agent.cineRecoveredText", "页面重新连接后已恢复后台结果：{{summary}}", { summary }) : summary,
                    detail: { kind: "cinematic", backendSessionId, status: "completed", recovered, completedAt },
                }),
                updatedAt: completedAt,
            };
        });
    };

    const failCinematicSession = (sessionId: string, backendSessionId: string, error: unknown) => {
        updateSession(sessionId, (session) => {
            const pending = session.pendingBackendSession;
            if (pending?.id !== backendSessionId) return session;
            const failedAt = new Date().toISOString();
            const text = error instanceof Error ? error.message : canvasT("videoCanvas.agent.cineFailed", "影视项目生成失败");
            return {
                ...session,
                pendingBackendSession: undefined,
                messages: upsertAssistantMessage(session.messages, {
                    id: pending.messageId,
                    role: "error",
                    title: canvasT("videoCanvas.agent.cineFailed", "影视项目生成失败"),
                    text,
                    detail: { kind: "cinematic", backendSessionId, status: "failed", failedAt },
                }),
                updatedAt: failedAt,
            };
        });
    };

    const runCinematicSession = async (sessionId: string, text: string, current: CanvasAgentSnapshot, config: AiConfig, onCreated?: (backendSessionId: string) => void) => {
        const requestConfig = resolveModelRequestConfig(config, config.textModel || config.model);
        const storyboardContext = resolveStoryboardGenerationContext(current.nodes);
        const controller = new AbortController();
        const requestKey = `creating:${nanoid()}`;
        let backendSessionId = "";
        cinematicSessionControllersRef.current.set(requestKey, controller);
        try {
            const detail = await createCinematicAgentSession(
                {
                    projectId,
                    prompt: text,
                    canvasSnapshot: compactSnapshot(current) as unknown as Record<string, unknown>,
                    projectStyle: storyboardContext.projectStyle,
                    characters: storyboardContext.characters,
                    config: backendAgentProviderConfig(requestConfig),
                },
                {
                    signal: controller.signal,
                    onCreated: (created) => {
                        backendSessionId = created.session.id;
                        cinematicSessionControllersRef.current.delete(requestKey);
                        cinematicSessionControllersRef.current.set(backendSessionId, controller);
                        setPendingCinematicSession(sessionId, backendSessionId);
                        addOnlineLog(canvasT("videoCanvas.agent.logCineSessionCreated", "后端影视 Agent 会话已创建"), { backendSessionId });
                        onCreated?.(backendSessionId);
                    },
                },
            );
            return { backendSessionId: detail.session.id, ops: requireOps(JSON.parse(cinematicAgentSessionOpsJson(detail))) };
        } catch (error) {
            if (backendSessionId && !isAgentSessionPollingAbort(error)) failCinematicSession(sessionId, backendSessionId, error);
            throw error;
        } finally {
            cinematicSessionControllersRef.current.delete(requestKey);
            if (backendSessionId) cinematicSessionControllersRef.current.delete(backendSessionId);
        }
    };

    const startChatSession = () => {
        if (activeSession && activeSession.messages.length === 0) {
            setLocalActiveSessionId(activeSession.id);
            return;
        }
        const session = createSession();
        setLocalSessions((prev) => [session, ...prev]);
        setLocalActiveSessionId(session.id);
    };

    const removeSessions = (ids: string[]) => {
        const next = safeSessions.filter((session) => !ids.includes(session.id));
        if (!next.length) {
            const session = createSession();
            setLocalSessions([session]);
            setLocalActiveSessionId(session.id);
        } else {
            setLocalSessions(next);
            setLocalActiveSessionId(localActiveSessionId && ids.includes(localActiveSessionId) ? next[0].id : localActiveSessionId);
        }
        cleanupImages({ sessions: next });
    };

    const clearSessions = () => {
        const session = createSession();
        setLocalSessions([session]);
        setLocalActiveSessionId(session.id);
        cleanupImages({ sessions: [session] });
    };

    const undoLastOnlineBatch = () => {
        const restored = onUndoOps();
        if (!restored) return;
        snapshotRef.current = restored;
        if (activeSession) appendMessage(activeSession.id, { id: nanoid(), role: "tool", title: canvasT("videoCanvas.agent.undoneTitle", "已撤销 Agent 批次"), text: canvasT("videoCanvas.agent.undoneText", "已恢复到本次写回前的画布状态"), detail: { status: "completed", remainingUndoCount: Math.max(0, undoOpsCount - 1) } });
    };

    const submit = async () => {
        const text = prompt.trim();
        if (!text || agentBusy) return;
        setPrompt("");
        await sendMessage(text, messages);
    };

    useEffect(() => {
        if (!cinematicEntry) return;
        // allo 不提供服务端影视 Agent：入口降级为普通在线 Agent 对话。
        setCinematicEntryActive(false);
        setView("chat");
        onAgentModeChange("online");
        onCinematicEntryConsumed?.();
    }, [cinematicEntry, onAgentModeChange, onCinematicEntryConsumed]);

    const submitCinematicProject = async (text: string) => {
        const value = text.trim();
        if (!value || agentBusy) return;
        setCinematicEntryActive(false);
        setPrompt("");
        await sendMessage(value, messages);
    };

    const resumePendingCinematicSession = async (sessionId: string, pending: CanvasAssistantPendingBackendSession) => {
        if (cinematicSessionControllersRef.current.has(pending.id)) return;
        const controller = new AbortController();
        cinematicSessionControllersRef.current.set(pending.id, controller);
        setIsRunning(true);
        addOnlineLog(canvasT("videoCanvas.agent.cineResumeLog", "恢复后端影视 Agent 会话"), { backendSessionId: pending.id });
        try {
            const detail = await resumeCinematicAgentSession(pending.id, { signal: controller.signal });
            const ops = requireOps(JSON.parse(cinematicAgentSessionOpsJson(detail)));
            executeOps(ops);
            completeCinematicSession(sessionId, pending.id, ops, true);
            addOnlineLog(canvasT("videoCanvas.agent.cineResumeDoneLog", "后端影视 Agent 会话恢复完成"), { backendSessionId: pending.id });
        } catch (error) {
            if (!isAgentSessionPollingAbort(error)) {
                failCinematicSession(sessionId, pending.id, error);
                addOnlineLog(canvasT("videoCanvas.agent.cineResumeFailLog", "后端影视 Agent 会话恢复失败"), error instanceof Error ? error.message : error);
            }
        } finally {
            if (cinematicSessionControllersRef.current.get(pending.id) === controller) cinematicSessionControllersRef.current.delete(pending.id);
            if (cinematicSessionControllersRef.current.size === 0) setIsRunning(false);
        }
    };

    useEffect(() => {
        localSessions.forEach((session) => {
            const pending = session.pendingBackendSession;
            if (pending?.kind === "cinematic" && pending.status === "pending") void resumePendingCinematicSession(session.id, pending);
        });
    }, [localSessions]);

    const addImagesToCanvas = (files: FileList | File[] | null) => {
        const file = Array.from(files || []).find((item) => item.type.startsWith("image/"));
        if (file) onPasteImage(file);
    };

    const collapse = () => {
        onCollapse();
    };

    const onlineContent = (
        <>
            <AgentPanelTabs
                value={view}
                theme={theme}
                items={[
                    { value: "setup", label: canvasT("videoCanvas.agent.tabSetup", "配置"), icon: <Settings2 className="size-3.5" /> },
                    { value: "chat", label: canvasT("videoCanvas.agent.tabChat", "对话"), icon: <MessageSquareText className="size-3.5" /> },
                    { value: "history", label: canvasT("videoCanvas.agent.tabHistory", "历史"), icon: <History className="size-3.5" />, count: historySessions.length },
                    { value: "log", label: canvasT("videoCanvas.agent.tabLog", "记录"), icon: <ScrollText className="size-3.5" />, count: onlineLogs.length },
                ]}
                onChange={setView}
                right={
                    <>
                        {view === "history" ? (
                            <Tooltip title={canvasT("videoCanvas.agent.deleteAll", "删除全部")}>
                                <Button type="text" shape="circle" className="!h-8 !w-8 !min-w-8" style={iconButtonStyle} icon={<X className="size-4" />} disabled={!historySessions.length} onClick={() => setDeleteChatIds(historySessions.map((session) => session.id))} />
                            </Tooltip>
                        ) : null}
                        <Tooltip title={canvasT("videoCanvas.agent.newChat", "新对话")}>
                            <Button
                                type="text"
                                shape="circle"
                                className="!h-8 !w-8 !min-w-8"
                                style={iconButtonStyle}
                                icon={<Plus className="size-4" />}
                                disabled={!hasMessages}
                                onClick={() => {
                                    startChatSession();
                                    setView("chat");
                                }}
                            />
                        </Tooltip>
                    </>
                }
            />

            {view === "setup" ? (
                <OnlineAgentSetupView theme={theme} activeModel={activeModel} onOpenConfig={() => navigateToSettings({ continueCreation: true })} />
            ) : (
                <div ref={chatListRef} className="thin-scrollbar min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4">
                    {view === "history" ? (
                        <AssistantHistory
                            sessions={historySessions}
                            activeSession={activeSession}
                            onOpen={(id) => {
                                setLocalActiveSessionId(id);
                                setView("chat");
                            }}
                            onDelete={(id) => setDeleteChatIds([id])}
                        />
                    ) : view === "log" ? (
                        <OnlineAgentLogView logs={onlineLogs} theme={theme} context={{ model: activeModel, running: agentBusy, confirmTools, messages: messages.length, nodes: snapshot.nodes.length, connections: snapshot.connections.length }} onClear={clearOnlineLogs} />
                    ) : (
                        <AssistantChatMessages
                            messages={messages}
                            theme={theme}
                            user={user}
                            busy={agentBusy}
                            nodeCount={contextSummary.nodeCount}
                            onSelectPrompt={setPrompt}
                            onRejectTool={stableRejectOnlineTool}
                            onApproveTool={stableApproveOnlineTool}
                        />
                    )}
                </div>
            )}

            {view === "chat" ? (
                <>
                    {selectedReferences.length ? (
                        <div className="thin-scrollbar flex max-w-full gap-1.5 overflow-x-auto px-3 pb-1">
                            {selectedReferences.map((item, index) => (
                                <AssistantReferenceChip
                                    key={item.id}
                                    item={item}
                                    label={assistantImageReferenceLabel(selectedReferences, index)}
                                    onRemove={() => {
                                        setRemovedReferenceIds((prev) => new Set(prev).add(item.id));
                                        if (selectedNodeIds.has(item.id)) onSelectNodeIds(new Set(Array.from(selectedNodeIds).filter((nodeId) => nodeId !== item.id)));
                                    }}
                                />
                            ))}
                        </div>
                    ) : null}
                    <AgentChatComposer
                        prompt={prompt}
                        sending={agentBusy}
                        placeholder={cinematicEntryActive ? canvasT("videoCanvas.agent.placeholderCinematic", "一句话描述题材、角色和核心冲突") : canvasT("videoCanvas.agent.placeholderChat", "描述你想让 Agent 如何操作画布")}
                        theme={theme}
                        onPromptChange={setPrompt}
                        onSubmit={cinematicEntryActive ? () => submitCinematicProject(prompt) : submit}
                        onAddFiles={addImagesToCanvas}
                        left={
                            <>
                                <AgentTextModelPicker config={effectiveConfig} value={effectiveConfig.textModel} onChange={(model) => updateConfig("textModel", model)} />
                                {cinematicEntryActive ? <span className="ml-2 inline-flex h-6 items-center rounded-md px-2 text-[var(--fs-tiny)] font-medium" style={{ background: theme.spatial.surface, color: theme.node.muted }}>{canvasT("videoCanvas.agent.cinematicBadge", "影视项目")}</span> : null}
                            </>
                        }
                    />
                </>
            ) : null}

            <Modal
                title={canvasT("videoCanvas.agent.deleteConfirmTitle", "删除对话记录？")}
                open={deleteChatIds.length > 0}
                centered
                onCancel={() => setDeleteChatIds([])}
                footer={
                    <>
                        <Button onClick={() => setDeleteChatIds([])}>{canvasT("videoCanvas.agent.cancel", "取消")}</Button>
                        <Button
                            danger
                            type="primary"
                            onClick={() => {
                                deleteChatIds.length === historySessions.length ? clearSessions() : removeSessions(deleteChatIds);
                                setDeleteChatIds([]);
                            }}
                        >
                            {canvasT("videoCanvas.agent.delete", "删除")}
                        </Button>
                    </>
                }
            >
                <p className="text-sm opacity-60">{canvasT("videoCanvas.agent.deleteConfirmBody", "将删除 {{count}} 条对话记录，此操作不可撤销。", { count: deleteChatIds.length })}</p>
            </Modal>
        </>
    );

    return (
        <motion.aside
            className="pointer-events-auto relative flex h-full w-full flex-col overflow-hidden rounded-[var(--panel-radius)] border"
            initial={{ x: 48, opacity: 0 }}
            animate={{ x: closing ? 28 : 0, opacity: closing ? 0 : 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: [0.22, 1, 0.36, 1] }}
            style={{
                borderColor: theme.toolbar.border,
                background: theme.spatial.elevated,
                color: theme.node.text,
                boxShadow: `0 24px 72px ${theme.spatial.shadow}`,
            }}
        >
                <AgentPanelChrome
                    theme={theme}
                    mode={agentMode}
                    context={contextSummary}
                    referenceCount={selectedReferences.length}
                    confirmTools={confirmTools}
                    canUndo={agentMode === "online" && canUndoOps}
                    undoCount={agentMode === "online" ? undoOpsCount : 0}
                    onModeChange={onAgentModeChange}
                    onConfirmToolsChange={(confirmTools) => setAgentState({ confirmTools })}
                    onUndo={undoLastOnlineBatch}
                    onCollapse={collapse}
                />
                {agentMode === "local" ? (
                    <CanvasLocalAgentPanel
                        embedded
                        snapshot={snapshot}
                        canUndoOps={canUndoOps}
                        undoOpsCount={undoOpsCount}
                        onApplyOps={onApplyOps}
                        onUndoOps={onUndoOps}
                        autoConnect={autoConnectLocal}
                    />
                ) : (
                    onlineContent
                )}
        </motion.aside>
    );
}

function backendAgentProviderConfig(config: ReturnType<typeof resolveModelRequestConfig>) {
    return {
        apiFormat: config.apiFormat,
        interfaceType: config.interfaceType,
        baseUrl: config.baseUrl,
        apiKey: config.apiKey,
        secretKey: config.secretKey,
        model: config.model,
        size: config.size,
        quality: config.quality,
        transparentBackground: config.transparentBackground,
        count: config.count,
        videoSeconds: config.videoSeconds,
        vquality: config.vquality,
        videoGenerateAudio: config.videoGenerateAudio,
        videoWatermark: config.videoWatermark,
        audioVoice: config.audioVoice,
        audioFormat: config.audioFormat,
        audioSpeed: config.audioSpeed,
        audioInstructions: config.audioInstructions,
        systemPrompt: config.systemPrompt,
    };
}

function cinematicSessionMessageId(backendSessionId: string) {
    return `cinematic-session:${backendSessionId}`;
}

function upsertAssistantMessage(messages: CanvasAssistantMessage[], message: CanvasAssistantMessage) {
    const exists = messages.some((item) => item.id === message.id);
    return exists ? messages.map((item) => (item.id === message.id ? { ...item, ...message } : item)) : [...messages, message];
}
