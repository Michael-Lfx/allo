import { lazy, Suspense } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
import { App } from "antd";
import { WorkspaceState } from "@oc/components/layout/workspace-state";
import { CanvasNodeInfoModal } from "@oc/components/canvas/canvas-node-toolbar";
import { CanvasSubtitleDialog } from "@oc/components/canvas/canvas-subtitle-dialog";
import { CanvasTimelineDialog } from "@oc/components/canvas/canvas-timeline-dialog";
import { CanvasCharacterReferenceModal } from "@oc/components/canvas/canvas-character-reference-modal";
import { CanvasVersionCompareModal } from "@oc/components/canvas/canvas-version-compare-modal";
import { CanvasProjectMediaDialogs } from "./canvas-project-media-dialogs";
import { CanvasProjectStatusDialogs } from "./canvas-project-status-dialogs";
import { AssetPickerModal } from "@oc/components/canvas/asset-picker-modal";
import { CanvasProjectAssetModal } from "@oc/components/canvas/canvas-project-asset-modal";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { flushCanvasStorePersistence } from "@oc/stores/canvas/use-canvas-store";
import { CanvasNodeType, type CanvasNodeData, type Position, type StoryboardColumn } from "@oc/types/canvas";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { ProjectDetail } from "@oc/services/api/projects";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { useCanvasNodeEditor } from "./use-canvas-node-editor";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";
import type { useCanvasStoryboard } from "./use-canvas-storyboard";
import type { useCanvasRenderModel } from "./use-canvas-render-model";
import type { useCanvasDirector } from "./use-canvas-director";
import type { useCanvasNodeOperations } from "./use-canvas-node-operations";
import type { useCanvasViewportController } from "./use-canvas-viewport-controller";
import type { useCanvasMediaTools } from "./use-canvas-media-tools";
import type { useCanvasGeneration } from "./use-canvas-generation";
import type { useCanvasAgentOperations } from "./use-canvas-agent-operations";
import type { CanvasRenderModel, CanvasAgentOps, CanvasAssistantState } from "./canvas-project-bundles";

const CanvasDirectorWorkbench = lazy(() => import("@oc/components/canvas/director/canvas-director-workbench").then((module) => ({ default: module.CanvasDirectorWorkbench })));
const CanvasDrawingEditorModal = lazy(() => import("@oc/components/canvas/canvas-drawing-editor-modal").then((module) => ({ default: module.CanvasDrawingEditorModal })));
const CanvasTextEditorModal = lazy(() => import("@oc/components/canvas/canvas-text-editor-modal").then((module) => ({ default: module.CanvasTextEditorModal })));
const CanvasScriptEditor = lazy(() => import("@oc/components/canvas/canvas-script-editor").then((module) => ({ default: module.CanvasScriptEditor })));
const CanvasLocalAgentPanel = lazy(() => import("@oc/components/canvas/canvas-local-agent-panel").then((module) => ({ default: module.CanvasLocalAgentPanel })));

type SetNodeId = Dispatch<SetStateAction<string | null>>;
type CanvasProjectDialogsProps = {
    imageInputRef: RefObject<HTMLInputElement | null>;
    handleImageInputChange: ReturnType<typeof useCanvasUpload>["handleImageInputChange"];
    handleConfigNodeChange: ReturnType<typeof useCanvasNodeEditor>["handleConfigNodeChange"];
    setInfoNodeId: SetNodeId;
    subtitleNodeId: string | null;
    nodeById: Map<string, CanvasNodeData>;
    setSubtitleNodeId: SetNodeId;
    timelineNodeId: string | null;
    nodes: CanvasNodeData[];
    currentProject: ReturnType<typeof useCanvasProjectLifecycle>["currentProject"];
    updateProject: ReturnType<typeof useCanvasProjectLifecycle>["updateProject"];
    setTimelineNodeId: SetNodeId;
    characterReferenceNode: CanvasNodeData | null;
    setCharacterReferenceNodeId: SetNodeId;
    textEditorNode: CanvasNodeData | null;
    setTextEditorNodeId: SetNodeId;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    drawingNode: CanvasNodeData | null;
    theme: CanvasTheme;
    projectId: string;
    setDrawingNodeId: SetNodeId;
    message: ReturnType<typeof App.useApp>["message"];
    replaceScriptRows: ReturnType<typeof useCanvasStoryboard>["replaceScriptRows"];
    setScriptEditorNodeId: SetNodeId;
    generateScriptImages: ReturnType<typeof useCanvasStoryboard>["generateScriptImages"];
    generateScriptVideos: ReturnType<typeof useCanvasStoryboard>["generateScriptVideos"];
    createAndGenerateScriptVideos: ReturnType<typeof useCanvasStoryboard>["createAndGenerateScriptVideos"];
    directorNodeId: string | null;
    setDirectorNodeId: SetNodeId;
    saveDirectorScene: ReturnType<typeof useCanvasDirector>["saveDirectorScene"];
    applyDirectorOutput: ReturnType<typeof useCanvasDirector>["applyDirectorOutput"];
    deleteNodes: ReturnType<typeof useCanvasNodeOperations>["deleteNodes"];
    directorOnboardingScope: string;
    versionCompareRootId: string | null;
    setVersionCompareRootId: SetNodeId;
    setPrimaryVersion: ReturnType<typeof useCanvasNodeOperations>["setPrimaryVersion"];
    focusCanvasNode: ReturnType<typeof useCanvasViewportController>["focusCanvasNode"];
    setCropNodeId: SetNodeId;
    setAnnotationNodeId: SetNodeId;
    setMaskEditNodeId: SetNodeId;
    setSplitNodeId: SetNodeId;
    setUpscaleNodeId: SetNodeId;
    setSuperResolveNodeId: SetNodeId;
    setPreviewNodeId: SetNodeId;
    cropImageNode: ReturnType<typeof useCanvasMediaTools>["cropImageNode"];
    saveAnnotatedImageNode: ReturnType<typeof useCanvasMediaTools>["saveAnnotatedImageNode"];
    maskEditImageNode: ReturnType<typeof useCanvasMediaTools>["maskEditImageNode"];
    splitImageNode: ReturnType<typeof useCanvasMediaTools>["splitImageNode"];
    upscaleImageNode: ReturnType<typeof useCanvasMediaTools>["upscaleImageNode"];
    extractVideoFrames: ReturnType<typeof useCanvasMediaTools>["extractVideoFrames"];
    closeFrameDialog: ReturnType<typeof useCanvasMediaTools>["closeFrameDialog"];
    frameDialogNodeId: ReturnType<typeof useCanvasMediaTools>["frameDialogNodeId"];
    taskDetail: ReturnType<typeof useCanvasGeneration>["taskDetail"];
    taskDetailLogs: ReturnType<typeof useCanvasGeneration>["taskDetailLogs"];
    taskDetailLoading: ReturnType<typeof useCanvasGeneration>["taskDetailLoading"];
    setTaskDetail: ReturnType<typeof useCanvasGeneration>["setTaskDetail"];
    clearConfirmOpen: boolean;
    setClearConfirmOpen: Dispatch<SetStateAction<boolean>>;
    clearCanvas: () => void;
    assetPickerOpen: boolean;
    handleAssetInsert: ReturnType<typeof useCanvasUpload>["handleAssetInsert"];
    closeAssetPicker: () => void;
    projectAssetOpen: boolean;
    linkedProjectQuery: { data?: ProjectDetail | undefined };
    projectAssetInitialCategory: string;
    closeProjectAssets: () => void;
    handleProjectAssetsInsert: ReturnType<typeof useCanvasUpload>["handleProjectAssetsInsert"];
    projectAssetInsertPosition: Position | undefined;
    codexCompactAgent: boolean;
    codexAutoConnect: boolean;
    renderModel: CanvasRenderModel;
    agentOps: CanvasAgentOps;
    assistant: CanvasAssistantState;
};

export function CanvasProjectDialogs(props: CanvasProjectDialogsProps) {
    const {
        imageInputRef,
        handleImageInputChange,
        handleConfigNodeChange,
        setInfoNodeId,
        subtitleNodeId,
        nodeById,
        setSubtitleNodeId,
        timelineNodeId,
        nodes,
        currentProject,
        updateProject,
        setTimelineNodeId,
        characterReferenceNode,
        setCharacterReferenceNodeId,
        textEditorNode,
        setTextEditorNodeId,
        setNodes,
        drawingNode,
        theme,
        projectId,
        setDrawingNodeId,
        message,
        replaceScriptRows,
        setScriptEditorNodeId,
        generateScriptImages,
        generateScriptVideos,
        createAndGenerateScriptVideos,
        directorNodeId,
        setDirectorNodeId,
        saveDirectorScene,
        applyDirectorOutput,
        deleteNodes,
        directorOnboardingScope,
        versionCompareRootId,
        setVersionCompareRootId,
        setPrimaryVersion,
        focusCanvasNode,
        setCropNodeId,
        setAnnotationNodeId,
        setMaskEditNodeId,
        setSplitNodeId,
        setUpscaleNodeId,
        setSuperResolveNodeId,
        setPreviewNodeId,
        cropImageNode,
        saveAnnotatedImageNode,
        maskEditImageNode,
        splitImageNode,
        upscaleImageNode,
        extractVideoFrames,
        closeFrameDialog,
        frameDialogNodeId,
        taskDetail,
        taskDetailLogs,
        taskDetailLoading,
        setTaskDetail,
        clearConfirmOpen,
        setClearConfirmOpen,
        clearCanvas,
        assetPickerOpen,
        handleAssetInsert,
        closeAssetPicker,
        projectAssetOpen,
        linkedProjectQuery,
        projectAssetInitialCategory,
        closeProjectAssets,
        handleProjectAssetsInsert,
        projectAssetInsertPosition,
        codexCompactAgent,
        codexAutoConnect,
        agentOps,
        assistant,
        renderModel,
    } = props;
    const { agentSnapshot, agentUndoCount, applyAgentOps, canUndoAgentOps, undoAgentOps } = agentOps;
    const { assistantMounted } = assistant;
    const { infoNode, activeScriptNode, activeDirectorScene, versionCompareNodes, cropNode, annotationNode, maskEditNode, splitNode, upscaleNode, superResolveNode, previewNode } = renderModel;
    return (
        <>
                    <input ref={imageInputRef} type="file" accept="image/*,video/*,audio/mpeg,audio/wav,audio/x-wav,.mp3,.wav" className="hidden" onChange={handleImageInputChange} />

                    <CanvasNodeInfoModal node={infoNode} open={Boolean(infoNode)} onClose={() => setInfoNodeId(null)} onMetadataChange={handleConfigNodeChange} />

                    {(() => {
                        const subtitleNode = subtitleNodeId ? nodeById.get(subtitleNodeId) : null;
                        return subtitleNode ? (
                            <CanvasSubtitleDialog
                                node={subtitleNode}
                                open
                                onClose={() => setSubtitleNodeId(null)}
                                onSave={handleConfigNodeChange}
                            />
                        ) : null;
                    })()}

                    <CanvasTimelineDialog
                        open={Boolean(timelineNodeId)}
                        seedNode={timelineNodeId ? nodeById.get(timelineNodeId) || null : null}
                        nodes={nodes}
                        timeline={currentProject?.timeline}
                        onClose={() => setTimelineNodeId(null)}
                        onSave={(timeline) => {
                            if (!currentProject) return;
                            updateProject(currentProject.id, { timeline });
                        }}
                    />

                    <CanvasCharacterReferenceModal node={characterReferenceNode} open={Boolean(characterReferenceNode)} onClose={() => setCharacterReferenceNodeId(null)} />

                    {textEditorNode ? (
                        <Suspense fallback={null}>
                            <CanvasTextEditorModal
                                node={textEditorNode}
                                open={Boolean(textEditorNode)}
                                onClose={() => setTextEditorNodeId(null)}
                                onSave={(nodeId, title, content, richText) => {
                                    setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, title, metadata: { ...node.metadata, content, richText } } : node)));
                                }}
                            />
                        </Suspense>
                    ) : null}

                    {drawingNode ? (
                        <Suspense
                            fallback={
                                <div className="fixed inset-0 z-[var(--z-toast)] grid place-items-center px-5" style={{ background: theme.canvas.background, color: theme.node.text }}>
                                    <WorkspaceState icon="loading" title={canvasT("videoCanvas.toast.loadingDrawingEditor", "正在加载绘图编辑器")} description={canvasT("videoCanvas.toast.preparingDrawing", "正在准备绘图画布。")} />
                                </div>
                            }
                        >
                            <CanvasDrawingEditorModal
                                node={drawingNode}
                                projectId={projectId}
                                open={Boolean(drawingNode)}
                                onClose={() => setDrawingNodeId(null)}
                                onSaved={(nodeId, summary) => {
                                    setNodes((current) =>
                                        current.map((node) =>
                                            node.id === nodeId
                                                ? {
                                                      ...node,
                                                      metadata: {
                                                          ...node.metadata,
                                                          drawingEngine: summary.engine,
                                                          drawingRevision: summary.revision,
                                                          drawingUpdatedAt: summary.updatedAt,
                                                          drawingShapeCount: summary.shapeCount,
                                                          drawingPageCount: summary.pageCount,
                                                      },
                                                  }
                                                : node,
                                        ),
                                    );
                                    message.success(canvasT("videoCanvas.toast.drawingSaved", "绘图已保存"));
                                }}
                            />
                        </Suspense>
                    ) : null}

                    {activeScriptNode ? (
                        <Suspense fallback={null}>
                            <CanvasScriptEditor
                                node={activeScriptNode}
                                open={Boolean(activeScriptNode)}
                                onClose={() => setScriptEditorNodeId(null)}
                                onUpdateRows={(rows) => activeScriptNode && replaceScriptRows(activeScriptNode.id, rows)}
                                onVisibleColumnsChange={(visibleColumns: StoryboardColumn[]) => {
                                    if (!activeScriptNode || !visibleColumns.length) return;
                                    setNodes((prev) =>
                                        prev.map((node) =>
                                            node.id === activeScriptNode.id
                                                ? { ...node, metadata: { ...node.metadata, storyboard: { rows: node.metadata?.storyboard?.rows || [], visibleColumns, referenceNodeIds: node.metadata?.storyboard?.referenceNodeIds || [] } } }
                                                : node,
                                        ),
                                    );
                                }}
                                onGenerateImages={(rowIds) => activeScriptNode && void generateScriptImages(activeScriptNode.id, rowIds)}
                                onGenerateVideos={(rowIds) => {
                                    if (!activeScriptNode) return;
                                    if (activeScriptNode.metadata?.storyboardVideoInputMode === "keyframe") void generateScriptVideos(activeScriptNode.id, rowIds);
                                    else void createAndGenerateScriptVideos(activeScriptNode.id, rowIds);
                                }}
                                onVideoInputModeChange={(storyboardVideoInputMode) => activeScriptNode && handleConfigNodeChange(activeScriptNode.id, { storyboardVideoInputMode })}
                            />
                        </Suspense>
                    ) : null}

                    {directorNodeId && activeDirectorScene ? (
                        <Suspense
                            fallback={
                                <div className="fixed inset-0 z-[var(--z-toast)] grid place-items-center px-5" style={{ background: theme.canvas.background, color: theme.node.text }}>
                                    <WorkspaceState icon="loading" title={canvasT("videoCanvas.toast.loadingDirector", "正在加载 3D 导演台")} description={canvasT("videoCanvas.toast.preparingDirector", "准备场景、镜头与空间控制。")} />
                                </div>
                            }
                        >
                            <CanvasDirectorWorkbench
                                open
                                scene={activeDirectorScene}
                                imageNodes={nodes.filter((node) => node.type === CanvasNodeType.Image && Boolean(node.metadata?.content))}
                                onClose={() => setDirectorNodeId(null)}
                                onChange={saveDirectorScene}
                                onApply={applyDirectorOutput}
                                onDeleteImageNode={(nodeId) => deleteNodes(new Set([nodeId]))}
                                onFlush={() => flushCanvasStorePersistence()}
                                onboardingScope={directorOnboardingScope}
                            />
                        </Suspense>
                    ) : null}

                    <CanvasVersionCompareModal
                        open={Boolean(versionCompareRootId)}
                        versions={versionCompareNodes}
                        onClose={() => setVersionCompareRootId(null)}
                        onSetPrimary={setPrimaryVersion}
                        onFocus={(nodeId) => {
                            setVersionCompareRootId(null);
                            focusCanvasNode(nodeId);
                        }}
                    />

                    <CanvasProjectMediaDialogs
                        cropNode={cropNode}
                        annotationNode={annotationNode}
                        maskEditNode={maskEditNode}
                        splitNode={splitNode}
                        upscaleNode={upscaleNode}
                        onCloseCrop={() => setCropNodeId(null)}
                        onCloseAnnotation={() => setAnnotationNodeId(null)}
                        onCloseMaskEdit={() => setMaskEditNodeId(null)}
                        onCloseSplit={() => setSplitNodeId(null)}
                        onCloseUpscale={() => setUpscaleNodeId(null)}
                        onCrop={(node, crop) => void cropImageNode(node, crop)}
                        onAnnotate={(node, dataUrl) => void saveAnnotatedImageNode(node, dataUrl)}
                        onMaskEdit={(node, payload) => void maskEditImageNode(node, payload)}
                        onSplit={(node, params) => void splitImageNode(node, params)}
                        onUpscale={(node, params) => void upscaleImageNode(node, params)}
                        frameNode={frameDialogNodeId ? nodeById.get(frameDialogNodeId) || null : null}
                        onCloseFrame={closeFrameDialog}
                        onExtractFrames={(node, params) => void extractVideoFrames(node, params)}
                    />

                    <CanvasProjectStatusDialogs
                        theme={theme}
                        task={taskDetail}
                        taskLogs={taskDetailLogs}
                        taskLoading={taskDetailLoading}
                        onCloseTask={() => setTaskDetail(null)}
                        superResolveNode={superResolveNode}
                        onCloseSuperResolve={() => setSuperResolveNodeId(null)}
                        previewNode={previewNode}
                        onClosePreview={() => setPreviewNodeId(null)}
                        clearConfirmOpen={clearConfirmOpen}
                        onCancelClear={() => setClearConfirmOpen(false)}
                        onConfirmClear={clearCanvas}
                    />

                    <AssetPickerModal open={assetPickerOpen} onInsert={handleAssetInsert} onClose={closeAssetPicker} />
                    <CanvasProjectAssetModal
                        open={projectAssetOpen}
                        detail={linkedProjectQuery.data}
                        initialCategory={projectAssetInitialCategory}
                        onClose={closeProjectAssets}
                        onInsert={(payloads) => handleProjectAssetsInsert(payloads, projectAssetInsertPosition)}
                    />
                    {codexCompactAgent && !assistantMounted ? (
                        <Suspense fallback={null}>
                            <CanvasLocalAgentPanel headless snapshot={agentSnapshot} canUndoOps={canUndoAgentOps} undoOpsCount={agentUndoCount} onApplyOps={applyAgentOps} onUndoOps={undoAgentOps} autoConnect={codexAutoConnect} />
                        </Suspense>
                    ) : null}
        </>
    );
}
