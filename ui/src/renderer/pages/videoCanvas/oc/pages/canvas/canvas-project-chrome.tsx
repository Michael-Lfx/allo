import type { Dispatch, RefObject, SetStateAction } from "react";
import { CanvasAgentChangeToast, CanvasMergeStatusToast, CanvasUploadStatusToast } from "./canvas-project-feedback";
import { CanvasNodeToolbar } from "@oc/components/canvas/canvas-node-toolbar";
import { Minimap } from "@oc/components/canvas/canvas-mini-map";
import { CanvasZoomControls } from "@oc/components/canvas/canvas-zoom-controls";
import { CanvasAssetTray } from "@oc/components/canvas/canvas-asset-tray";
import { CanvasProjectContextMenu } from "./canvas-project-context-menu";
import { HideWhileNodeDragging } from "./canvas-project-world-layers";
import { CanvasNodeType, type CanvasNodeData, type CanvasWorkspaceMode, type ContextMenuState, type Position, type ViewportTransform } from "@oc/types/canvas";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasTrayMediaAsset } from "./use-canvas-render-model";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { useCanvasMediaTools } from "./use-canvas-media-tools";
import type { useCanvasAgentOperations } from "./use-canvas-agent-operations";
import type { useCanvasNodeEditor } from "./use-canvas-node-editor";
import type { useCanvasNodeOperations } from "./use-canvas-node-operations";
import type { useCanvasViewportController } from "./use-canvas-viewport-controller";
import type { useCanvasGenerationRetry } from "./use-canvas-generation-retry";
import type { useCanvasDirector } from "./use-canvas-director";
import type { useCanvasHistory } from "./use-canvas-history";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";
import type { CanvasRenderModel, CanvasHistoryActions, CanvasAgentOps } from "./canvas-project-bundles";

type SetNodeId = Dispatch<SetStateAction<string | null>>;
type CanvasProjectCanvasChromeProps = {
    theme: CanvasTheme;
    uploadStatus: ReturnType<typeof useCanvasUpload>["uploadStatus"];
    mergeVideoProgress: ReturnType<typeof useCanvasMediaTools>["mergeVideoProgress"];
    nodeImageSettingsOpen: boolean;
    emotionNodeId: string | null;
    workspaceMode: CanvasWorkspaceMode;
    viewport: ViewportTransform;
    containerRef: RefObject<HTMLDivElement | null>;
    keepNodeToolbar: (nodeId: string) => void;
    hideNodeToolbar: () => void;
    openTextNodeEditor: (node: CanvasNodeData) => void;
    openDrawingNode: (node: CanvasNodeData) => void;
    setInfoNodeId: SetNodeId;
    handleFontSizeChange: ReturnType<typeof useCanvasNodeEditor>["handleFontSizeChange"];
    setDialogNodeId: SetNodeId;
    generateImageFromTextNode: (node: CanvasNodeData) => void;
    handleUploadRequest: ReturnType<typeof useCanvasUpload>["handleUploadRequest"];
    downloadNodeImage: ReturnType<typeof useCanvasNodeEditor>["downloadNodeImage"];
    saveNodeAsset: ReturnType<typeof useCanvasNodeEditor>["saveNodeAsset"];
    setAnnotationNodeId: SetNodeId;
    setMaskEditNodeId: SetNodeId;
    setEmotionNodeId: SetNodeId;
    generatePortraitTextureNode: ReturnType<typeof useCanvasMediaTools>["generatePortraitTextureNode"];
    setCropNodeId: SetNodeId;
    setSplitNodeId: SetNodeId;
    setUpscaleNodeId: SetNodeId;
    setSuperResolveNodeId: SetNodeId;
    setAngleNodeId: SetNodeId;
    setPreviewNodeId: SetNodeId;
    extractVideoLastFrame: ReturnType<typeof useCanvasMediaTools>["extractVideoLastFrame"];
    extractingVideoFrameNodeId: string | null;
    createImageReversePromptNodes: ReturnType<typeof useCanvasMediaTools>["createImageReversePromptNodes"];
    handleRetryNode: ReturnType<typeof useCanvasGenerationRetry>;
    toggleNodeFreeResize: ReturnType<typeof useCanvasNodeEditor>["toggleNodeFreeResize"];
    toggleNodeLocked: ReturnType<typeof useCanvasNodeOperations>["toggleNodeLocked"];
    setSubtitleNodeId: SetNodeId;
    setTimelineNodeId: SetNodeId;
    deleteNodes: ReturnType<typeof useCanvasNodeOperations>["deleteNodes"];
    isMiniMapOpen: boolean;
    focusMode: boolean;
    nodes: CanvasNodeData[];
    size: { width: number; height: number };
    previewViewport: ReturnType<typeof useCanvasViewportController>["previewViewport"];
    handleViewportChange: (viewport: ViewportTransform) => void;
    setZoomScale: ReturnType<typeof useCanvasViewportController>["setZoomScale"];
    resetViewport: () => void;
    setIsMiniMapOpen: Dispatch<SetStateAction<boolean>>;
    setShortcutRequestNonce: Dispatch<SetStateAction<number>>;
    currentProject: ReturnType<typeof useCanvasProjectLifecycle>["currentProject"];
    selectedNodeIds: Set<string>;
    createMediaAssetNode: ReturnType<typeof useCanvasUpload>["createMediaAssetNode"];
    focusCanvasImageNode: ReturnType<typeof useCanvasViewportController>["focusCanvasImageNode"];
    contextMenu: ContextMenuState | null;
    shortDramaEnabled: boolean;
    hasCopiedNodes: boolean;
    screenToCanvas: ReturnType<typeof useCanvasViewportController>["screenToCanvas"];
    setContextMenu: Dispatch<SetStateAction<ContextMenuState | null>>;
    createNode: ReturnType<typeof useCanvasNodeOperations>["createNode"];
    createFolder: ReturnType<typeof useCanvasNodeOperations>["createFolder"];
    setStylePickerOpen: Dispatch<SetStateAction<boolean>>;
    createDirectorShot: ReturnType<typeof useCanvasDirector>["createDirectorShot"];
    openAssetsAtPosition: ReturnType<typeof useCanvasUpload>["openAssetsAtPosition"];
    openProjectAssets: (initialCategory?: string, position?: Position) => void;
    pasteAtPosition: (position: Position) => void;
    copyNodesToClipboard: ReturnType<typeof useCanvasNodeOperations>["copyNodesToClipboard"];
    duplicateNode: ReturnType<typeof useCanvasNodeOperations>["duplicateNode"];
    deleteConnection: ReturnType<typeof useCanvasNodeOperations>["deleteConnection"];
    copyNodeContentToClipboard: (node: CanvasNodeData | null) => Promise<void>;
    copyNodeMediaUrlToClipboard: (node: CanvasNodeData | null) => Promise<void>;
    handleConfigNodeChange: ReturnType<typeof useCanvasNodeEditor>["handleConfigNodeChange"];
    toggleFrameCollapsed: (nodeId: string) => void;
    renderModel: CanvasRenderModel;
    historyActions: CanvasHistoryActions;
    agentOps: CanvasAgentOps;
};

export function CanvasProjectCanvasChrome(props: CanvasProjectCanvasChromeProps) {
    const {
        theme,
        uploadStatus,
        mergeVideoProgress,
        nodeImageSettingsOpen,
        emotionNodeId,
        workspaceMode,
        viewport,
        containerRef,
        keepNodeToolbar,
        hideNodeToolbar,
        openTextNodeEditor,
        openDrawingNode,
        setInfoNodeId,
        handleFontSizeChange,
        setDialogNodeId,
        generateImageFromTextNode,
        handleUploadRequest,
        downloadNodeImage,
        saveNodeAsset,
        setAnnotationNodeId,
        setMaskEditNodeId,
        setEmotionNodeId,
        generatePortraitTextureNode,
        setCropNodeId,
        setSplitNodeId,
        setUpscaleNodeId,
        setSuperResolveNodeId,
        setAngleNodeId,
        setPreviewNodeId,
        extractVideoLastFrame,
        extractingVideoFrameNodeId,
        createImageReversePromptNodes,
        handleRetryNode,
        toggleNodeFreeResize,
        toggleNodeLocked,
        setSubtitleNodeId,
        setTimelineNodeId,
        deleteNodes,
        isMiniMapOpen,
        focusMode,
        nodes,
        size,
        previewViewport,
        handleViewportChange,
        setZoomScale,
        resetViewport,
        setIsMiniMapOpen,
        setShortcutRequestNonce,
        currentProject,
        selectedNodeIds,
        createMediaAssetNode,
        focusCanvasImageNode,
        contextMenu,
        shortDramaEnabled,
        hasCopiedNodes,
        screenToCanvas,
        setContextMenu,
        createNode,
        createFolder,
        setStylePickerOpen,
        createDirectorShot,
        openAssetsAtPosition,
        openProjectAssets,
        pasteAtPosition,
        copyNodesToClipboard,
        duplicateNode,
        deleteConnection,
        copyNodeContentToClipboard,
        copyNodeMediaUrlToClipboard,
        handleConfigNodeChange,
        toggleFrameCollapsed,
        historyActions,
        agentOps,
        renderModel,
    } = props;
    const { historyState, undoCanvas, redoCanvas } = historyActions;
    const { lastAgentChange, viewLastAgentChange, undoAgentOps, dismissLastAgentChange } = agentOps;
    const { toolbarNode, mediaAssets, canvasMediaNodes, contextMenuNode } = renderModel;
    return (
        <>
                    {uploadStatus ? <CanvasUploadStatusToast status={uploadStatus} theme={theme} /> : null}
                    {mergeVideoProgress ? <CanvasMergeStatusToast progress={mergeVideoProgress} theme={theme} /> : null}
                    {lastAgentChange ? (
                        <CanvasAgentChangeToast
                            change={lastAgentChange}
                            theme={theme}
                            onView={viewLastAgentChange}
                            onUndo={() => {
                                undoAgentOps();
                            }}
                            onClose={dismissLastAgentChange}
                        />
                    ) : null}

                    <HideWhileNodeDragging>
                    <CanvasNodeToolbar
                        node={nodeImageSettingsOpen || emotionNodeId ? null : toolbarNode}
                        workspaceMode={workspaceMode}
                        viewport={viewport}
                        containerRef={containerRef}
                        onKeep={keepNodeToolbar}
                        onLeave={hideNodeToolbar}
                        onInfo={(node) => (node.metadata?.workflowKind === "character" && node.metadata.characterAssetId ? openTextNodeEditor(node) : setInfoNodeId(node.id))}
                        onEditText={openTextNodeEditor}
                        onDecreaseFont={(node) => handleFontSizeChange(node.id, Math.max(10, (node.metadata?.fontSize || 14) - 2))}
                        onIncreaseFont={(node) => handleFontSizeChange(node.id, Math.min(32, (node.metadata?.fontSize || 14) + 2))}
                        onToggleDialog={(node) => setDialogNodeId((current) => (current === node.id ? null : node.id))}
                        onGenerateImage={generateImageFromTextNode}
                        onUpload={(node) => handleUploadRequest(node.id)}
                        onDownload={(node) => void downloadNodeImage(node)}
                        onSaveAsset={(node) => void saveNodeAsset(node)}
                        onAnnotate={(node) => setAnnotationNodeId(node.id)}
                        onMaskEdit={(node) => setMaskEditNodeId(node.id)}
                        onEmotion={(node) => {
                            setDialogNodeId(null);
                            setEmotionNodeId((current) => (current === node.id ? null : node.id));
                        }}
                        onPortraitTexture={generatePortraitTextureNode}
                        onCrop={(node) => setCropNodeId(node.id)}
                        onSplit={(node) => setSplitNodeId(node.id)}
                        onUpscale={(node) => setUpscaleNodeId(node.id)}
                        onSuperResolve={(node) => setSuperResolveNodeId(node.id)}
                        onAngle={(node) => {
                            setDialogNodeId(null);
                            setAngleNodeId((current) => (current === node.id ? null : node.id));
                        }}
                        onViewImage={(node) => setPreviewNodeId(node.id)}
                        onExtractVideoLastFrame={(node) => void extractVideoLastFrame(node)}
                        extractingVideoFrame={toolbarNode?.id === extractingVideoFrameNodeId}
                        onReversePrompt={createImageReversePromptNodes}
                        onRetry={(node) => void handleRetryNode(node)}
                        onToggleFreeResize={(node) => toggleNodeFreeResize(node.id)}
                        onToggleLocked={(node) => toggleNodeLocked(node.id)}
                        onSubtitles={(node) => setSubtitleNodeId(node.id)}
                        onTimeline={(node) => setTimelineNodeId(node.id)}
                        onDelete={(node) => deleteNodes(new Set([node.id]))}
                    />
                    </HideWhileNodeDragging>

                    {isMiniMapOpen && !focusMode ? <Minimap nodes={nodes} viewport={viewport} viewportSize={size} canvasContainerRef={containerRef} onViewportPreviewChange={previewViewport} onViewportChange={handleViewportChange} /> : null}

                    {!focusMode ? (
                        <div
                            data-canvas-no-zoom
                            className="absolute bottom-[calc(var(--canvas-inset-y)+var(--space-16))] left-4 z-[var(--z-panel)] flex items-end gap-2 lg:bottom-[var(--canvas-inset-y)]"
                            onMouseDown={(event) => event.stopPropagation()}
                            onPointerDown={(event) => event.stopPropagation()}
                            onWheel={(event) => event.stopPropagation()}
                        >
                            <CanvasZoomControls
                                scale={viewport.k}
                                containerRef={containerRef}
                                onScaleChange={setZoomScale}
                                onReset={resetViewport}
                                isMiniMapOpen={isMiniMapOpen}
                                onToggleMiniMap={() => setIsMiniMapOpen((value) => !value)}
                                onOpenShortcuts={() => setShortcutRequestNonce((value) => value + 1)}
                            />
                            <CanvasAssetTray
                                mediaAssets={mediaAssets}
                                canvasMediaNodes={canvasMediaNodes}
                                showLibrary={!currentProject?.projectId}
                                activeNodeId={selectedNodeIds.size === 1 ? Array.from(selectedNodeIds)[0] : null}
                                onInsertMediaAsset={(asset) => void createMediaAssetNode(asset)}
                                onFocusCanvasMedia={focusCanvasImageNode}
                            />
                        </div>
                    ) : null}

                    <CanvasProjectContextMenu
                        menu={contextMenu}
                        node={contextMenuNode}
                        workspaceMode={workspaceMode}
                        isProjectLinked={Boolean(shortDramaEnabled && currentProject?.projectId)}
                        canUndo={historyState.canUndo}
                        canRedo={historyState.canRedo}
                        canPaste={hasCopiedNodes || Boolean(navigator.clipboard)}
                        screenToCanvas={screenToCanvas}
                        onClose={() => setContextMenu(null)}
                        onAddNode={(type, position) => createNode(type, position)}
                        onAddFolder={(position) => createFolder(position)}
                        onChooseStyle={() => setStylePickerOpen(true)}
                        onOpenDirector={createDirectorShot}
                        onUpload={(nodeId, position) => handleUploadRequest(nodeId, position)}
                        onOpenAssets={openAssetsAtPosition}
                        onOpenProjectCharacters={(position) => openProjectAssets("character", position)}
                        onUndo={undoCanvas}
                        onRedo={redoCanvas}
                        onPaste={pasteAtPosition}
                        onCopyNode={(nodeId) => copyNodesToClipboard(new Set([nodeId]))}
                        onDuplicate={duplicateNode}
                        onDeleteNode={(nodeId) => deleteNodes(new Set([nodeId]))}
                        onDeleteConnection={deleteConnection}
                        onSaveAsset={(node) => {
                            void saveNodeAsset(node);
                        }}
                        onViewMedia={(node) => setPreviewNodeId(node.id)}
                        onEditText={openTextNodeEditor}
                        onOpenDrawing={openDrawingNode}
                        onGenerateImage={generateImageFromTextNode}
                        onCopyContent={(node) => {
                            void copyNodeContentToClipboard(node);
                        }}
                        onCopyMediaUrl={(node) => {
                            void copyNodeMediaUrlToClipboard(node);
                        }}
                        onSetAssetCategory={(nodeId, assetCategory) => handleConfigNodeChange(nodeId, { assetCategory })}
                        onToggleFrame={(node) => toggleFrameCollapsed(node.id)}
                    />
        </>
    );
}
