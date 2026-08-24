import { lazy, Suspense } from "react";
import type { Dispatch, SetStateAction } from "react";
import { AssistantPanelColumn } from "./canvas-assistant-panel-column";
import type { CanvasNodeData, CanvasAssistantSession } from "@oc/types/canvas";
import type { CanvasAgentMode } from "@oc/components/canvas/canvas-agent-chat-ui";
import type { useCanvasAgentOperations } from "./use-canvas-agent-operations";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { CanvasAgentOps, CanvasAssistantState } from "./canvas-project-bundles";

const CanvasAssistantPanel = lazy(() => import("@oc/components/canvas/canvas-assistant-panel").then((module) => ({ default: module.CanvasAssistantPanel })));

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
    cinematicAgentEntry: boolean;
    setCinematicAgentEntry: Dispatch<SetStateAction<boolean>>;
    agentOps: CanvasAgentOps;
    assistant: CanvasAssistantState;
};

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
        cinematicAgentEntry,
        setCinematicAgentEntry,
        agentOps,
        assistant,
    } = props;
    const { agentSnapshot, agentUndoCount, applyAgentOps, canUndoAgentOps, undoAgentOps } = agentOps;
    const { agentMode, assistantMounted, assistantClosing, setAgentMode, closeAgent } = assistant;
    return (
        <>
                        {assistantMounted ? (
                            <AssistantPanelColumn width={assistantWidth} closing={assistantClosing} topInset={focusMode ? "0px" : "var(--canvas-topbar-offset)"} onWidthChange={setAssistantWidth}>
                                {(resizing) => (
                                    <Suspense fallback={null}>
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
                                            cinematicEntry={cinematicAgentEntry}
                                            onCinematicEntryConsumed={() => setCinematicAgentEntry(false)}
                                            resizing={resizing}
                                        />
                                    </Suspense>
                                )}
                            </AssistantPanelColumn>
                        ) : null}
        </>
    );
}