import { useTranslation } from "react-i18next";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { History, MessageSquareText, Plus, ScrollText, X } from "lucide-react";
import { Button, Modal, Tooltip } from "antd";
import { motion } from "motion/react";

import { useConfigStore, useEffectiveConfig } from "@oc/stores/use-config-store";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { nanoid } from "nanoid";
import { useAssetStore } from "@oc/stores/use-asset-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import { summarizeCanvasContext } from "@oc/lib/canvas/canvas-context-summary";
import { AgentChatComposer, AgentPanelTabs, type CanvasAgentMode } from "./canvas-agent-chat-ui";
import { AgentPanelChrome } from "./canvas-agent-panel-chrome";
import { CANVAS_AGENT_PANEL_MOTION_MS } from "./canvas-assistant-panel-motion";
import { type CanvasAssistantMessage, type CanvasAssistantSession, type CanvasNodeData } from "@oc/types/canvas";
import { useCanvasAgentStore } from "@oc/stores/canvas/use-canvas-agent-store";
import { type CanvasAgentOp, type CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { createSession, useCanvasOnlineAgentLoop } from "./canvas-online-agent-loop";
import { AgentTextModelPicker, AssistantChatMessages, AssistantHistory, AssistantReferenceChip, OnlineAgentLogView, assistantImageReferenceLabel, buildAgentComposerReferences, buildAssistantReferences } from "./canvas-assistant-panel-views";

const PANEL_MOTION_SECONDS = CANVAS_AGENT_PANEL_MOTION_MS / 1000;
type OnlineAgentTab = "chat" | "history" | "log";
const startedHomeAgentKeys = new Set<string>();

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
    agentMode?: CanvasAgentMode;
    onAgentModeChange?: (mode: CanvasAgentMode) => void;
    autoConnectLocal?: boolean;
    closing: boolean;
    onCollapse: () => void;
    onExtractFrames?: (nodeId: string, timesMs: number[]) => Promise<{ createdNodeIds: string[]; message: string }>;
    resizing?: boolean;
    autoStart?: { prompt: string; modelContext?: string } | null;
    onAutoStartConsumed?: () => void;
    appearImmediately?: boolean;
};

export function CanvasAssistantPanel({ nodes, selectedNodeIds, snapshot, sessions, activeSessionId, onSelectNodeIds, onSessionsChange, onApplyOps, canUndoOps, undoOpsCount, onUndoOps, onPasteImage, closing, onCollapse, onExtractFrames, resizing = false, autoStart, onAutoStartConsumed, appearImmediately = false }: CanvasAssistantPanelProps) {
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
    const [deleteChatIds, setDeleteChatIds] = useState<string[]>([]);
    const [removedReferenceIds, setRemovedReferenceIds] = useState<Set<string>>(new Set());
    const [localSessions, setLocalSessions] = useState<CanvasAssistantSession[]>(() => (sessions.length ? sessions : [createSession()]));
    const [localActiveSessionId, setLocalActiveSessionId] = useState<string | null>(activeSessionId);
    const applyingExternalSessionsRef = useRef(false);
    const chatListRef = useRef<HTMLDivElement>(null);
    const snapshotRef = useRef(snapshot);
    const safeSessions = localSessions.length ? localSessions : [createSession()];
    const activeSession = useMemo(() => safeSessions.find((session) => session.id === localActiveSessionId) || safeSessions[0] || null, [localActiveSessionId, safeSessions]);
    const historySessions = safeSessions.filter((session) => session.messages.length > 0);
    const messages = activeSession?.messages || [];
    const hasMessages = messages.length > 0;
    const activeModel = effectiveConfig.textModel || effectiveConfig.model;
    const selectedNodeKey = useMemo(() => Array.from(selectedNodeIds).sort().join(","), [selectedNodeIds]);
    const allSelectedReferences = useMemo(() => buildAssistantReferences(nodes, selectedNodeIds), [nodes, selectedNodeIds]);
    const selectedReferences = useMemo(() => allSelectedReferences.filter((item) => !removedReferenceIds.has(item.id)), [allSelectedReferences, removedReferenceIds]);
    const mentionReferences = useMemo(() => buildAgentComposerReferences(nodes), [nodes]);
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

    const { isRunning, agentActivity, onlineLogs, clearOnlineLogs, sendMessage, approveOnlineTool, rejectOnlineTool } = useCanvasOnlineAgentLoop({
        snapshotRef,
        config: effectiveConfig,
        confirmTools,
        selectedReferences,
        activeSession,
        onApplyOps,
        onExtractFrames,
        appendMessage,
        upsertMessage,
        getMessageById: (messageId) => safeSessions.flatMap((session) => session.messages).find((item) => item.id === messageId),
        getSessionByMessageId: (messageId) => safeSessions.find((session) => session.messages.some((item) => item.id === messageId)),
        activateSession: (session) => {
            setLocalSessions((prev) => (prev.some((item) => item.id === session.id) ? prev : [...prev, session]));
            setLocalActiveSessionId(session.id);
        },
    });

    const agentBusy = isRunning || safeSessions.some((session) => session.pendingBackendSession?.status === "pending");
    const autoStartTriedRef = useRef(false);

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

    useEffect(() => {
        if (applyingExternalSessionsRef.current) {
            applyingExternalSessionsRef.current = false;
            return;
        }
        if (sessions === localSessions && activeSessionId === localActiveSessionId) return;
        onSessionsChange(localSessions, localActiveSessionId);
    }, [activeSessionId, localActiveSessionId, localSessions, onSessionsChange, sessions]);

    useEffect(() => {
        if (view !== "chat") return;
        const frame = requestAnimationFrame(() => chatListRef.current?.scrollTo({ top: chatListRef.current.scrollHeight }));
        return () => cancelAnimationFrame(frame);
    }, [agentBusy, localActiveSessionId, messages, view]);

    useEffect(() => {
        setRemovedReferenceIds(new Set());
    }, [selectedNodeKey]);

    useEffect(() => {
        if (!autoStart?.prompt.trim() || agentBusy) return;
        if (messages.some((message) => message.role === "user")) {
            onAutoStartConsumed?.();
            return;
        }
        const startKey = `${snapshot.projectId}::${autoStart.prompt.trim()}`;
        if (startedHomeAgentKeys.has(startKey) || autoStartTriedRef.current) {
            onAutoStartConsumed?.();
            return;
        }
        autoStartTriedRef.current = true;
        startedHomeAgentKeys.add(startKey);
        void sendMessage(autoStart.prompt.trim(), messages, undefined, { modelContext: autoStart.modelContext, skipConfirm: true }).then((sent) => {
            if (sent) {
                onAutoStartConsumed?.();
                return;
            }
            autoStartTriedRef.current = false;
            startedHomeAgentKeys.delete(startKey);
        });
    }, [agentBusy, autoStart, messages, onAutoStartConsumed, sendMessage, snapshot.projectId]);

    // 稳定回调身份：配合 memo(AgentChatMessage) 避免任一状态变化重渲全部历史消息。
    const rejectOnlineToolRef = useRef(rejectOnlineTool);
    const approveOnlineToolRef = useRef(approveOnlineTool);
    useEffect(() => {
        rejectOnlineToolRef.current = rejectOnlineTool;
        approveOnlineToolRef.current = approveOnlineTool;
    });
    const stableRejectOnlineTool = useCallback((id: string) => rejectOnlineToolRef.current(id), []);
    const stableApproveOnlineTool = useCallback((id: string) => approveOnlineToolRef.current(id), []);

    const startChatSession = () => {
        if (agentBusy) return;
        if (activeSession && activeSession.messages.length === 0) {
            setLocalActiveSessionId(activeSession.id);
            setView("chat");
            return;
        }
        const session = createSession();
        setLocalSessions((prev) => [session, ...prev.filter((item) => item.messages.length > 0 || item.id === session.id)]);
        setLocalActiveSessionId(session.id);
        setPrompt("");
        setView("chat");
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
                        <Tooltip title={hasMessages ? canvasT("videoCanvas.agent.newChat", "新对话") : canvasT("videoCanvas.agent.newChatCurrentEmpty", "当前已是空白对话")}>
                            <Button
                                type="text"
                                className="!h-8 !px-2"
                                style={iconButtonStyle}
                                icon={<Plus className="size-3.5" />}
                                disabled={!hasMessages || agentBusy}
                                onClick={startChatSession}
                            >
                                {canvasT("videoCanvas.agent.newChat", "新对话")}
                            </Button>
                        </Tooltip>
                    </>
                }
            />

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
                        activity={agentActivity}
                        nodeCount={contextSummary.nodeCount}
                        onSelectPrompt={setPrompt}
                        onRejectTool={stableRejectOnlineTool}
                        onApproveTool={stableApproveOnlineTool}
                    />
                )}
            </div>

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
                        placeholder={canvasT("videoCanvas.agent.placeholderChat", "描述你想让 Agent 如何操作画布，用 @ 引用节点")}
                        theme={theme}
                        mentionReferences={mentionReferences}
                        onPromptChange={setPrompt}
                        onSubmit={submit}
                        onAddFiles={addImagesToCanvas}
                        left={<AgentTextModelPicker config={effectiveConfig} value={effectiveConfig.textModel} onChange={(model) => updateConfig("textModel", model)} />}
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
            initial={appearImmediately ? false : { x: 48, opacity: 0 }}
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
                    context={contextSummary}
                    referenceCount={selectedReferences.length}
                    confirmTools={confirmTools}
                    canUndo={canUndoOps}
                    undoCount={undoOpsCount}
                    onConfirmToolsChange={(confirmTools) => setAgentState({ confirmTools })}
                    onUndo={undoLastOnlineBatch}
                    onCollapse={collapse}
                />
                {onlineContent}
        </motion.aside>
    );
}
