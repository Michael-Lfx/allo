import type { ComponentProps, Dispatch, ReactNode, RefObject, SetStateAction } from "react";
import { useMemo } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent, DragEvent as ReactDragEvent } from "react";
import { InfiniteCanvas } from "@oc/components/canvas/infinite-canvas";
import { CanvasLeaferGraphicsLayer } from "@oc/components/canvas/canvas-leafer-graphics-layer";
import { CanvasNodeActionContext } from "@oc/components/canvas/canvas-node-action-context";
import { CanvasNodeGraphContext } from "@oc/components/canvas/canvas-node-graph-context";
import { CanvasProjectWorldLayers } from "./canvas-project-world-layers";
import { CanvasActiveTaskPanel } from "@oc/components/canvas/canvas-active-task-panel";
import { CanvasFocusModeBar } from "@oc/components/canvas/canvas-focus-mode-bar";
import { CanvasFileDropOverlay } from "@oc/components/canvas/canvas-file-drop-overlay";
import { CanvasToolbar } from "@oc/components/canvas/canvas-toolbar";
import { getContextResourceNodes } from "@oc/lib/canvas/canvas-resource-references";
import { CanvasNodeType, type CanvasNodeMetadata, type CanvasToolMode, type CanvasWorkspaceMode, type Position, type ViewportTransform } from "@oc/types/canvas";
import type { CanvasBackgroundMode, CanvasTheme } from "@oc/lib/canvas-theme";
import type { GenerationTask } from "@oc/services/api/task-center";
import type { useCanvasAssistantVisibility } from "./use-canvas-assistant-visibility";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { useCanvasHistory } from "./use-canvas-history";
import type { useCanvasNodeOperations } from "./use-canvas-node-operations";
import type { CanvasRenderModel, CanvasHistoryActions, CanvasAssistantState } from "./canvas-project-bundles";

type CanvasProjectStageProps = Omit<ComponentProps<typeof CanvasProjectWorldLayers>, "connectionLayerBounds" | "displayConnections" | "nodeById" | "visibleNodes" | "frameChildrenById" | "batchChildCountById" | "batchMotionById" | "reduceMediaEffects" | "resourceReferenceByNodeId" | "mentionReferencesByNodeId" | "selectedNodeBounds"> & {
    viewport: ViewportTransform;
    theme: CanvasTheme;
    emotionNodeId: string | null;
    containerRef: RefObject<HTMLDivElement | null>;
    backgroundMode: CanvasBackgroundMode;
    handleViewportChange: (viewport: ViewportTransform) => void;
    handleViewportPreviewChange: (viewport: ViewportTransform) => void;
    handleCanvasMouseDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
    canvasTool: CanvasToolMode;
    handleCanvasDoubleClick: (event: ReactMouseEvent<HTMLDivElement>) => void;
    deselectCanvas: () => void;
    handleCanvasContextMenu: (event: ReactMouseEvent) => void;
    handleDrop: (event: ReactDragEvent<HTMLDivElement>) => void;
    handleFileDragEnter: (event: ReactDragEvent<HTMLDivElement>) => void;
    handleFileDragLeave: (event: ReactDragEvent<HTMLDivElement>) => void;
    handleFileDragOver: (event: ReactDragEvent<HTMLDivElement>) => void;
    activeTasks: GenerationTask[];
    focusMode: boolean;
    focusDockRevealed: boolean;
    setFocusDockRevealed: Dispatch<SetStateAction<boolean>>;
    exitFocusMode: () => void;
    zoomCanvasIn: () => void;
    zoomCanvasOut: () => void;
    resetViewport: () => void;
    fileDropActive: boolean;
    emptyCanvasState: ReactNode;
    workspaceMode: CanvasWorkspaceMode;
    setCanvasTool: Dispatch<SetStateAction<CanvasToolMode>>;
    shortDramaEnabled: boolean;
    currentProject: ReturnType<typeof useCanvasProjectLifecycle>["currentProject"];
    showImageInfo: boolean;
    setBackgroundMode: Dispatch<SetStateAction<CanvasBackgroundMode>>;
    setShowImageInfo: Dispatch<SetStateAction<boolean>>;
    createNode: ReturnType<typeof useCanvasNodeOperations>["createNode"];
    createFolder: ReturnType<typeof useCanvasNodeOperations>["createFolder"];
    setDirectorTemplateRequest: Dispatch<SetStateAction<{ position?: Position } | null>>;
    handleUploadRequest: ReturnType<typeof useCanvasUpload>["handleUploadRequest"];
    deleteNodes: ReturnType<typeof useCanvasNodeOperations>["deleteNodes"];
    setClearConfirmOpen: Dispatch<SetStateAction<boolean>>;
    openAssetsAtPosition: ReturnType<typeof useCanvasUpload>["openAssetsAtPosition"];
    openProjectAssets: (initialCategory?: string, position?: Position) => void;
    setStylePickerOpen: Dispatch<SetStateAction<boolean>>;
    renderModel: CanvasRenderModel;
    historyActions: CanvasHistoryActions;
    assistant: CanvasAssistantState;
    updateNodeMetadata: (nodeId: string, patch: CanvasNodeMetadata) => void;
};

export function CanvasProjectStage(props: CanvasProjectStageProps) {
    const {
        projectId,
        selectedConnectionId,
        connections,
        scriptScrollTopById,
        selectedNodeIds,
        showImageInfo,
        collapsingBatchIds,
        openingBatchIds,
        emotionNodeId,
        batchSourceNodeIds,
        batchConnectionPreview,
        selectionBoundsElementRef,
        renderCanvasNodeContent,
        onConnectionSelect,
        onConnectionContextMenu,
        onNodeMouseDown,
        onNodeHoverStart,
        onNodeHoverEnd,
        onConnectStart,
        onNodeResize,
        onToggleFrame,
        onNodeTitleChange,
        onNodeContextMenu,
        onNodeContentChange,
        onToggleBatch,
        onSetBatchPrimary,
        onRetry,
        onReloadResource,
        onCancelTask,
        onOpenTaskDetails,
        onOpenVersions,
        onViewImage,
        onReplaceMedia,
        onOpenTextEditor,
        onOpenDirector,
        onOpenDrawing,
        onStartBatchConnection,
        viewport,
        theme,
        containerRef,
        backgroundMode,
        handleViewportChange,
        handleViewportPreviewChange,
        handleCanvasMouseDown,
        canvasTool,
        handleCanvasDoubleClick,
        deselectCanvas,
        handleCanvasContextMenu,
        handleDrop,
        handleFileDragEnter,
        handleFileDragLeave,
        handleFileDragOver,
        activeTasks,
        focusMode,
        focusDockRevealed,
        setFocusDockRevealed,
        exitFocusMode,
        zoomCanvasIn,
        zoomCanvasOut,
        resetViewport,
        fileDropActive,
        emptyCanvasState,
        workspaceMode,
        setCanvasTool,
        shortDramaEnabled,
        currentProject,
        setBackgroundMode,
        setShowImageInfo,
        createNode,
        createFolder,
        setDirectorTemplateRequest,
        handleUploadRequest,
        deleteNodes,
        setClearConfirmOpen,
        openAssetsAtPosition,
        openProjectAssets,
        setStylePickerOpen,
        renderModel,
        historyActions,
        assistant,
        updateNodeMetadata,
    } = props;
    const { historyState, undoCanvas, redoCanvas } = historyActions;
    const { assistantOpen, closeAgent, openAgent } = assistant;
    const { connectionLayerBounds, displayConnections, nodeById, visibleNodes, frameChildrenById, batchChildCountById, batchMotionById, reduceMediaEffects, resourceReferenceByNodeId, mentionReferencesByNodeId, selectedNodeBounds } = renderModel;
    const graphNodes = useMemo(() => Array.from(nodeById.values()), [nodeById]);
    const nodeGraphContext = useMemo(
        () => ({ getUpstreamNodes: (nodeId: string) => getContextResourceNodes(nodeId, graphNodes, connections) }),
        [connections, graphNodes],
    );
    const nodeActionContext = useMemo(
        () => ({
            updateMetadata: updateNodeMetadata,
            // 图片 onLoad 比例校正：经 onNodeResize 但 markManual:false，避免写成 manualSize。
            resizeNode: (nodeId: string, size: { width: number; height: number }) => {
                const node = nodeById.get(nodeId);
                if (!node || node.metadata?.locked) return;
                if (Math.abs(node.width - size.width) < 1 && Math.abs(node.height - size.height) < 1) return;
                onNodeResize(nodeId, size.width, size.height, {
                    x: node.position.x + node.width / 2 - size.width / 2,
                    y: node.position.y + node.height / 2 - size.height / 2,
                }, { markManual: false });
            },
        }),
        [nodeById, onNodeResize, updateNodeMetadata],
    );
    return (
        <>
                        <div className="relative min-w-0 flex-1 overflow-hidden">
                            <InfiniteCanvas
                                containerRef={containerRef}
                                viewport={viewport}
                                backgroundMode={backgroundMode}
                                graphicsLayer={
                                    <CanvasLeaferGraphicsLayer
                                        containerRef={containerRef}
                                        viewport={viewport}
                                        theme={theme}
                                        displayConnections={displayConnections}
                                        selectedConnectionId={selectedConnectionId}
                                        connections={connections}
                                        selectedNodeIds={selectedNodeIds}
                                        scriptScrollTopById={scriptScrollTopById}
                                        nodeById={nodeById}
                                        selectedNodeBounds={selectedNodeBounds}
                                        batchConnectionPreview={batchConnectionPreview}
                                    />
                                }
                                onViewportChange={handleViewportChange}
                                onViewportPreviewChange={handleViewportPreviewChange}
                                onCanvasMouseDown={handleCanvasMouseDown}
                                boxSelectEnabled={canvasTool === "box-select"}
                                onCanvasDoubleClick={handleCanvasDoubleClick}
                                onCanvasDeselect={deselectCanvas}
                                onContextMenu={handleCanvasContextMenu}
                                onDrop={handleDrop}
                                onFileDragEnter={handleFileDragEnter}
                                onFileDragLeave={handleFileDragLeave}
                                onFileDragOver={handleFileDragOver}
                            >
                                <CanvasNodeActionContext.Provider value={nodeActionContext}>
                                <CanvasNodeGraphContext.Provider value={nodeGraphContext}>
                                <CanvasProjectWorldLayers
                                    projectId={projectId}
                                    connectionLayerBounds={connectionLayerBounds}
                                    displayConnections={displayConnections}
                                    selectedConnectionId={selectedConnectionId}
                                    connections={connections}
                                    scriptScrollTopById={scriptScrollTopById}
                                    nodeById={nodeById}
                                    visibleNodes={visibleNodes}
                                    frameChildrenById={frameChildrenById}
                                    selectedNodeIds={selectedNodeIds}
                                    batchChildCountById={batchChildCountById}
                                    collapsingBatchIds={collapsingBatchIds}
                                    openingBatchIds={openingBatchIds}
                                    batchMotionById={batchMotionById}
                                    showImageInfo={showImageInfo}
                                    reduceMediaEffects={reduceMediaEffects}
                                    resourceReferenceByNodeId={resourceReferenceByNodeId}
                                    mentionReferencesByNodeId={mentionReferencesByNodeId}
                                    mediaEffectsDisabledNodeId={emotionNodeId}
                                    selectedNodeBounds={selectedNodeBounds}
                                    batchSourceNodeIds={batchSourceNodeIds}
                                    batchConnectionPreview={batchConnectionPreview}
                                    selectionBoundsElementRef={selectionBoundsElementRef}
                                    renderCanvasNodeContent={renderCanvasNodeContent}
                                    onConnectionSelect={onConnectionSelect}
                                    onConnectionContextMenu={onConnectionContextMenu}
                                    onNodeMouseDown={onNodeMouseDown}
                                    onNodeHoverStart={onNodeHoverStart}
                                    onNodeHoverEnd={onNodeHoverEnd}
                                    onConnectStart={onConnectStart}
                                    onNodeResize={onNodeResize}
                                    onToggleFrame={onToggleFrame}
                                    onNodeTitleChange={onNodeTitleChange}
                                    onNodeContextMenu={onNodeContextMenu}
                                    onNodeContentChange={onNodeContentChange}
                                    onToggleBatch={onToggleBatch}
                                    onSetBatchPrimary={onSetBatchPrimary}
                                    onRetry={onRetry}
                                    onReloadResource={onReloadResource}
                                    onCancelTask={onCancelTask}
                                    onOpenTaskDetails={onOpenTaskDetails}
                                    onOpenVersions={onOpenVersions}
                                    onViewImage={onViewImage}
                                    onReplaceMedia={onReplaceMedia}
                                    onOpenTextEditor={onOpenTextEditor}
                                    onOpenDirector={onOpenDirector}
                                    onOpenDrawing={onOpenDrawing}
                                    onStartBatchConnection={onStartBatchConnection}
                                />
                                </CanvasNodeGraphContext.Provider>
                                </CanvasNodeActionContext.Provider>
                            </InfiniteCanvas>

                            <CanvasActiveTaskPanel tasks={activeTasks} />

                            {focusMode ? (
                                <CanvasFocusModeBar
                                    dockRevealed={focusDockRevealed}
                                    agentOpen={assistantOpen}
                                    zoomPercent={viewport.k}
                                    onToggleDock={() => setFocusDockRevealed((value) => !value)}
                                    onToggleAgent={() => (assistantOpen ? closeAgent() : openAgent())}
                                    onExit={exitFocusMode}
                                    onZoomIn={zoomCanvasIn}
                                    onZoomOut={zoomCanvasOut}
                                    onFit={resetViewport}
                                />
                            ) : null}

                            <CanvasFileDropOverlay active={fileDropActive} theme={theme} />

                            {emptyCanvasState}

                            {!focusMode || focusDockRevealed ? (
                                <CanvasToolbar
                                    selectedCount={selectedNodeIds.size}
                                    workspaceMode={workspaceMode}
                                    canvasTool={canvasTool}
                                    onToolChange={setCanvasTool}
                                    isProjectLinked={Boolean(shortDramaEnabled && currentProject?.projectId)}
                                    canUndo={historyState.canUndo}
                                    canRedo={historyState.canRedo}
                                    backgroundMode={backgroundMode}
                                    showImageInfo={showImageInfo}
                                    onAddImage={() => createNode(CanvasNodeType.Image)}
                                    onAddVideo={() => createNode(CanvasNodeType.Video)}
                                    onAddAudio={() => createNode(CanvasNodeType.Audio)}
                                    onAddText={() => createNode(CanvasNodeType.Text)}
                                    onChooseStyle={() => setStylePickerOpen(true)}
                                    onAddScript={() => createNode(CanvasNodeType.Script)}
                                    onAddFrame={() => createNode(CanvasNodeType.Frame)}
                                    onAddFolder={createFolder}
                                    onAddDrawing={() => createNode(CanvasNodeType.Drawing)}
                                    onOpenDirector={() => setDirectorTemplateRequest({})}
                                    onAddExtensionNode={(type) => createNode(type)}
                                    onUndo={undoCanvas}
                                    onRedo={redoCanvas}
                                    onUpload={() => handleUploadRequest()}
                                    onDelete={() => deleteNodes(new Set(selectedNodeIds))}
                                    onClear={() => setClearConfirmOpen(true)}
                                    onDeselect={deselectCanvas}
                                    onBackgroundModeChange={setBackgroundMode}
                                    onShowImageInfoChange={setShowImageInfo}
                                    onOpenMyAssets={() => {
                                        openAssetsAtPosition();
                                    }}
                                    onOpenProjectCharacters={() => openProjectAssets("character")}
                                />
                            ) : null}
                        </div>
        </>
    );
}
