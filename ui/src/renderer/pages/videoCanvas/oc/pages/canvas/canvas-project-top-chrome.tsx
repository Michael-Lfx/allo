import type { Dispatch, RefObject, SetStateAction } from "react";
import { CanvasTopBar } from "./canvas-project-top-bar";
import { CanvasNodeSearchModal } from "@oc/components/canvas/canvas-node-search-modal";
import { CanvasShortDramaGuide } from "@oc/components/canvas/canvas-short-drama-entry";
import { CanvasStylePickerModal } from "@oc/components/canvas/canvas-style-picker-modal";
import { CanvasDirectorTemplateModal } from "@oc/components/canvas/director/canvas-director-template-modal";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { summarizeCanvasContext } from "@oc/lib/canvas/canvas-context-summary";
import type { CanvasNodeData, CanvasMediaPerformanceMode, Position } from "@oc/types/canvas";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";
import type { useCanvasHistory } from "./use-canvas-history";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { useCanvasAssistantVisibility } from "./use-canvas-assistant-visibility";
import type { useCanvasShortDrama } from "./use-canvas-short-drama";
import type { useCanvasStyleWorkflow } from "./use-canvas-style-workflow";
import type { useCanvasProjectShare } from "./use-canvas-project-share";
import type { useCanvasDirector } from "./use-canvas-director";
import type { CanvasHistoryActions, CanvasAssistantState } from "./canvas-project-bundles";

type CanvasProjectTopChromeProps = {
    focusMode: boolean;
    currentProject: ReturnType<typeof useCanvasProjectLifecycle>["currentProject"];
    titleDraft: string;
    setTitleDraft: Dispatch<SetStateAction<string>>;
    titleEditing: boolean;
    setTitleEditing: Dispatch<SetStateAction<boolean>>;
    startTitleEditing: () => void;
    finishTitleEditing: () => void;
    createAndOpenProject: ReturnType<typeof useCanvasProjectLifecycle>["createAndOpenProject"];
    deleteCurrentProject: ReturnType<typeof useCanvasProjectLifecycle>["deleteCurrentProject"];
    handleUploadRequest: ReturnType<typeof useCanvasUpload>["handleUploadRequest"];
    codexCompactAgent: boolean;
    localAgentConnected: boolean;
    localAgentEnabled: boolean;
    localAgentActivity: string;
    shortcutRequestNonce: number;
    mediaPerformanceMode: CanvasMediaPerformanceMode;
    setMediaPerformanceMode: Dispatch<SetStateAction<CanvasMediaPerformanceMode>>;
    setNodeSearchOpen: Dispatch<SetStateAction<boolean>>;
    shortDramaEnabled: boolean;
    canvasContext: ReturnType<typeof summarizeCanvasContext>;
    linkedProjectQuery: { data?: { project: { name: string } } | null | undefined };
    enterFocusMode: () => void;
    shortDramaGuide: { progress: ReturnType<typeof useCanvasShortDrama>["progress"]; collapsed: boolean; onToggle: () => void } | undefined;
    nodeSearchOpen: boolean;
    nodes: CanvasNodeData[];
    nodeById: Map<string, CanvasNodeData>;
    toggleFrameCollapsed: (nodeId: string) => void;
    toggleBatchExpanded: (nodeId: string) => void;
    selectedNodeIdsRef: RefObject<Set<string>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    focusCanvasNode: (nodeId: string) => void;
    skipShortDramaGuide: () => void;
    activateShortDramaStep: ReturnType<typeof useCanvasShortDrama>["activateStep"];
    stylePickerOpen: boolean;
    setStylePickerOpen: Dispatch<SetStateAction<boolean>>;
    directorTemplateRequest: { position?: Position } | null;
    setDirectorTemplateRequest: Dispatch<SetStateAction<{ position?: Position } | null>>;
    createDirectorShot: ReturnType<typeof useCanvasDirector>["createDirectorShot"];
    activeStylePresetId: string | undefined;
    selectCanvasStyle: ReturnType<typeof useCanvasStyleWorkflow>["selectCanvasStyle"];
    historyActions: CanvasHistoryActions;
    assistant: CanvasAssistantState;
    projectShare: ReturnType<typeof useCanvasProjectShare>;
};

export function CanvasProjectTopChrome(props: CanvasProjectTopChromeProps) {
    const {
        focusMode,
        currentProject,
        titleDraft,
        setTitleDraft,
        titleEditing,
        setTitleEditing,
        startTitleEditing,
        finishTitleEditing,
        createAndOpenProject,
        deleteCurrentProject,
        handleUploadRequest,
        codexCompactAgent,
        localAgentConnected,
        localAgentEnabled,
        localAgentActivity,
        shortcutRequestNonce,
        mediaPerformanceMode,
        setMediaPerformanceMode,
        setNodeSearchOpen,
        shortDramaEnabled,
        canvasContext,
        linkedProjectQuery,
        enterFocusMode,
        shortDramaGuide,
        nodeSearchOpen,
        nodes,
        nodeById,
        toggleFrameCollapsed,
        toggleBatchExpanded,
        selectedNodeIdsRef,
        setSelectedNodeIds,
        setSelectedConnectionId,
        focusCanvasNode,
        skipShortDramaGuide,
        activateShortDramaStep,
        stylePickerOpen,
        setStylePickerOpen,
        directorTemplateRequest,
        setDirectorTemplateRequest,
        createDirectorShot,
        activeStylePresetId,
        selectCanvasStyle,
        historyActions,
        assistant,
        projectShare,
    } = props;
    const { historyState, undoCanvas, redoCanvas } = historyActions;
    const { assistantOpen, closeAgent, openAgent } = assistant;
    return (
        <>
                    {!focusMode ? (
                        <CanvasTopBar
                            title={currentProject?.title || canvasT("videoCanvas.chrome.untitled", "未命名画布")}
                            titleDraft={titleDraft}
                            isTitleEditing={titleEditing}
                            onTitleDraftChange={setTitleDraft}
                            onStartTitleEditing={startTitleEditing}
                            onFinishTitleEditing={finishTitleEditing}
                            onCancelTitleEditing={() => setTitleEditing(false)}
                            canUndo={historyState.canUndo}
                            canRedo={historyState.canRedo}
                            onCreateProject={createAndOpenProject}
                            onDeleteProject={deleteCurrentProject}
                            onImportImage={() => handleUploadRequest()}
                            onUndo={undoCanvas}
                            onRedo={redoCanvas}
                            agentOpen={assistantOpen}
                            compactAgentStatus={codexCompactAgent ? { connected: localAgentConnected, enabled: localAgentEnabled, activity: localAgentActivity } : undefined}
                            onToggleAgent={() => (assistantOpen ? closeAgent() : openAgent())}
                            shortcutRequestNonce={shortcutRequestNonce}
                            mediaPerformanceMode={mediaPerformanceMode}
                            onMediaPerformanceModeChange={setMediaPerformanceMode}
                            onOpenSearch={() => setNodeSearchOpen(true)}
                            projectContext={
                                shortDramaEnabled && currentProject?.projectId
                                    ? {
                                          ...canvasContext,
                                          projectId: currentProject.projectId,
                                          projectName: linkedProjectQuery.data?.project.name || currentProject.title,
                                      }
                                    : undefined
                            }
                            onEnterFocusMode={enterFocusMode}
                            shortDramaGuide={shortDramaGuide}
                            onExportProject={() => void projectShare.exportProject()}
                            onPublishTvShow={() => void projectShare.publishToTvShow()}
                            exporting={projectShare.exporting}
                            publishing={projectShare.publishing}
                        />
                    ) : null}

                    <CanvasNodeSearchModal
                        open={nodeSearchOpen}
                        nodes={nodes}
                        onClose={() => setNodeSearchOpen(false)}
                        onFocus={(nodeId) => {
                            const target = nodeById.get(nodeId);
                            const parent = target?.parentId ? nodeById.get(target.parentId) : null;
                            if (parent?.metadata?.frame?.collapsed) toggleFrameCollapsed(parent.id);
                            const batchRoot = target?.metadata?.batchRootId ? nodeById.get(target.metadata.batchRootId) : null;
                            if (batchRoot && !batchRoot.metadata?.imageBatchExpanded) toggleBatchExpanded(batchRoot.id);
                            const selection = new Set([nodeId]);
                            selectedNodeIdsRef.current = selection;
                            setSelectedNodeIds(selection);
                            setSelectedConnectionId(null);
                            focusCanvasNode(nodeId);
                        }}
                    />

                    {!focusMode && shortDramaGuide ? (
                        <CanvasShortDramaGuide progress={shortDramaGuide.progress} collapsed={shortDramaGuide.collapsed} onToggle={shortDramaGuide.onToggle} onSkip={skipShortDramaGuide} onStepClick={activateShortDramaStep} />
                    ) : null}

                    <CanvasStylePickerModal open={stylePickerOpen} value={activeStylePresetId} onClose={() => setStylePickerOpen(false)} onSelect={selectCanvasStyle} />
                    <CanvasDirectorTemplateModal
                        open={Boolean(directorTemplateRequest)}
                        onClose={() => setDirectorTemplateRequest(null)}
                        onSelect={(templateId) => createDirectorShot(templateId, directorTemplateRequest?.position)}
                    />
        </>
    );
}
