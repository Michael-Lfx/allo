import { lazy, Suspense } from "react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import { Bot, LoaderCircle } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { CanvasNodeData, CanvasAssistantSession } from "@oc/types/canvas";
import { loadCanvasAssistantPanel } from "@renderer/pages/videoCanvas/loadAssistantPanel";
import { AssistantPanelColumn } from "./canvas-assistant-panel-column";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { CanvasAgentOps, CanvasAssistantState } from "./canvas-project-bundles";

const CanvasAssistantPanel = lazy(() => loadCanvasAssistantPanel().then((module) => ({ default: module.CanvasAssistantPanel })));

type CanvasProjectAssistantColumnProps = {
    assistantWidth: number;
    setAssistantWidth: Dispatch<SetStateAction<number>>;
    focusMode: boolean;
    nodes: CanvasNodeData[];
    selectedNodeIds: Set<string>;
    projectId: string;
    chatSessions: CanvasAssistantSession[];
    activeChatId: string | null;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    handleAssistantSessionsChange: (sessions: CanvasAssistantSession[], activeId: string | null) => void;
    pasteAssistantImage: ReturnType<typeof useCanvasUpload>["pasteAssistantImage"];
    codexAutoConnect: boolean;
    extractFramesForAgent: (nodeId: string, timesMs: number[]) => Promise<{ createdNodeIds: string[]; message: string }>;
    agentOps: CanvasAgentOps;
    assistant: CanvasAssistantState;
    autoStart?: { prompt: string; modelContext?: string; meta?: string } | null;
    onAutoStartConsumed?: () => void;
    modelCatalogReady?: boolean;
};

function CanvasAssistantPanelFallback({ busy }: { busy?: boolean }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    return (
        <aside
            className="pointer-events-auto relative flex h-full w-full flex-col overflow-hidden rounded-[var(--panel-radius)] border"
            style={{
                borderColor: theme.toolbar.border,
                background: theme.spatial.elevated,
                color: theme.node.text,
                boxShadow: `0 24px 72px ${theme.spatial.shadow}`,
            }}
            aria-busy={busy || undefined}
            aria-live="polite"
        >
            <header className="shrink-0 px-3 pb-2 pt-3">
                <div className="flex min-w-0 items-center gap-2.5">
                    <span className="grid size-9 shrink-0 place-items-center rounded-md" style={{ background: theme.accent.primarySoft, color: theme.accent.primary }}>
                        <Bot className="size-[18px]" />
                    </span>
                    <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-semibold leading-5">Agent</div>
                        <div className="truncate text-[var(--fs-label)] leading-4" style={{ color: theme.node.muted }}>
                            {busy
                                ? canvasT("videoCanvas.agent.working", "正在推演...")
                                : canvasT("videoCanvas.agent.collab", "画布协作")}
                        </div>
                    </div>
                </div>
            </header>
            <div className="flex min-h-0 flex-1 items-center justify-center gap-2 px-4" style={{ color: theme.node.muted }}>
                <LoaderCircle className="size-4 animate-spin" />
                <span className="text-sm">{canvasT("videoCanvas.agent.working", "正在推演...")}</span>
            </div>
        </aside>
    );
}

export function CanvasProjectAssistantColumn(props: CanvasProjectAssistantColumnProps) {
    const {
        assistantWidth,
        setAssistantWidth,
        focusMode,
        nodes,
        selectedNodeIds,
        projectId,
        chatSessions,
        activeChatId,
        setSelectedNodeIds,
        handleAssistantSessionsChange,
        pasteAssistantImage,
        codexAutoConnect,
        extractFramesForAgent,
        agentOps,
        assistant,
        autoStart,
        onAutoStartConsumed,
        modelCatalogReady,
    } = props;
    const { agentSnapshot, agentUndoCount, applyAgentOps, canUndoAgentOps, undoAgentOps } = agentOps;
    const { agentMode, assistantMounted, assistantClosing, setAgentMode, closeAgent } = assistant;
    return (
        <>
                        {assistantMounted ? (
                            <AssistantPanelColumn width={assistantWidth} closing={assistantClosing} topInset={focusMode ? "0px" : "var(--canvas-topbar-offset)"} onWidthChange={setAssistantWidth}>
                                {(resizing) => (
                                    <Suspense fallback={<CanvasAssistantPanelFallback busy={Boolean(autoStart)} />}>
                                        <CanvasAssistantPanel
                                            nodes={nodes}
                                            selectedNodeIds={selectedNodeIds}
                                            snapshot={agentSnapshot}
                                            projectId={projectId}
                                            sessions={chatSessions}
                                            activeSessionId={activeChatId}
                                            onSelectNodeIds={setSelectedNodeIds}
                                            onSessionsChange={handleAssistantSessionsChange}
                                            onApplyOps={applyAgentOps}
                                            canUndoOps={canUndoAgentOps}
                                            undoOpsCount={agentUndoCount}
                                            onUndoOps={undoAgentOps}
                                            onPasteImage={pasteAssistantImage}
                                            agentMode={agentMode}
                                            onAgentModeChange={setAgentMode}
                                            autoConnectLocal={codexAutoConnect}
                                            closing={assistantClosing}
                                            onCollapse={closeAgent}
                                            onExtractFrames={extractFramesForAgent}
                                            resizing={resizing}
                                            autoStart={autoStart}
                                            onAutoStartConsumed={onAutoStartConsumed}
                                            appearImmediately={Boolean(autoStart)}
                                            modelCatalogReady={modelCatalogReady}
                                        />
                                    </Suspense>
                                )}
                            </AssistantPanelColumn>
                        ) : null}
        </>
    );
}
