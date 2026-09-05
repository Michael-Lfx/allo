import type { Dispatch, RefObject, SetStateAction } from "react";
import { App } from "antd";
import { WorkspaceState } from "@oc/components/layout/workspace-state";
import { CanvasNodeInfoModal } from "@oc/components/canvas/canvas-node-toolbar";
import { CanvasSubtitleDialog } from "@oc/components/canvas/canvas-subtitle-dialog";
import { CanvasTimelineDialog } from "@oc/components/canvas/canvas-timeline-dialog";
import { CanvasCharacterReferenceModal } from "@oc/components/canvas/canvas-character-reference-modal";
import { CanvasVersionCompareModal } from "@oc/components/canvas/canvas-version-compare-modal";
import { CanvasLazyEditor } from "@oc/components/canvas/canvas-lazy-editor";
import { CanvasProjectMediaDialogs } from "./canvas-project-media-dialogs";
import { CanvasProjectStatusDialogs } from "./canvas-project-status-dialogs";
import { CanvasProjectAssetModal } from "@oc/components/canvas/canvas-project-asset-modal";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import { resourceStorageKey } from "@oc/services/api/resources";
import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { flushCanvasStorePersistence } from "@oc/stores/canvas/use-canvas-store";
import { AiArtCritiqueModal } from "@oc/components/canvas/art-critique/ai-art-critique-modal";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type Position, type StoryboardColumn } from "@oc/types/canvas";
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

function loadDirectorWorkbench() {
    return import("@oc/components/canvas/director/canvas-director-workbench").then((module) => ({ default: module.CanvasDirectorWorkbench }));
}
function loadDrawingEditor() {
    return import("@oc/components/canvas/canvas-drawing-editor-modal").then((module) => ({ default: module.CanvasDrawingEditorModal }));
}
function loadTextEditor() {
    return import("@oc/components/canvas/canvas-text-editor-modal").then((module) => ({ default: module.CanvasTextEditorModal }));
}
function loadScriptEditor() {
    return import("@oc/components/canvas/canvas-script-editor").then((module) => ({ default: module.CanvasScriptEditor }));
}
function loadLocalAgentPanel() {
    return import("@oc/components/canvas/canvas-local-agent-panel").then((module) => ({ default: module.CanvasLocalAgentPanel }));
}

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
    connections: CanvasConnection[];
    artCritiqueNodeId: string | null;
    setArtCritiqueNodeId: SetNodeId;
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
        connections,
        artCritiqueNodeId,
        setArtCritiqueNodeId,
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
                        onExportMedia={(meta) => {
                            const spec = NODE_DEFAULT_SIZE[CanvasNodeType.Video];
                            const seed = timelineNodeId ? nodeById.get(timelineNodeId) : null;
                            const created = createCanvasNode(
                                CanvasNodeType.Video,
                                {
                                    x: (seed?.position.x || 160) + (seed?.width || 0) + 96 + spec.width / 2,
                                    y: (seed?.position.y || 160) + spec.height / 2,
                                },
                                {
                                    content: meta.url,
                                    storageKey: resourceStorageKey(meta.media_id),
                                    mediaId: meta.media_id,
                                    mimeType: meta.mime,
                                    bytes: meta.bytes,
                                    durationMs: meta.duration_ms ?? undefined,
                                    naturalWidth: meta.width ?? undefined,
                                    naturalHeight: meta.height ?? undefined,
                                    status: "success",
                                    workflowKind: "final",
                                    workflowTitle: "时间线成片",
                                    videoEditOperation: "concat",
                                },
                            );
                            created.title = meta.title || canvasT("videoCanvas.timeline.export", "导出成片");
                            setNodes((current) => [...current, created]);
                        }}
                    />

                    {(() => {
                        const artCritiqueNode = artCritiqueNodeId ? nodeById.get(artCritiqueNodeId) || null : null;
                        const artCritiqueInputs = artCritiqueNode
                            ? connections
                                  .filter((connection) => connection.toNodeId === artCritiqueNode.id)
                                  .sort((left, right) => left.id.localeCompare(right.id))
                                  .map((connection) => nodeById.get(connection.fromNodeId))
                                  .filter((node): node is CanvasNodeData => Boolean(node))
                            : [];
                        return (
                            <AiArtCritiqueModal
                                node={artCritiqueNode}
                                upstreamNodes={artCritiqueInputs}
                                open={Boolean(artCritiqueNode)}
                                onClose={() => setArtCritiqueNodeId(null)}
                                onUpdateState={(nodeId, state) => handleConfigNodeChange(nodeId, { artCritique: state })}
                            />
                        );
                    })()}

                    <CanvasCharacterReferenceModal node={characterReferenceNode} open={Boolean(characterReferenceNode)} onClose={() => setCharacterReferenceNodeId(null)} />

                    {textEditorNode ? (
                        <CanvasLazyEditor
                            load={loadTextEditor}
                            errorTitle="文本编辑器"
                            node={textEditorNode}
                            open={Boolean(textEditorNode)}
                            onClose={() => setTextEditorNodeId(null)}
                            onSave={(nodeId, title, content, richText) => {
                                setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, title, metadata: { ...node.metadata, content, richText } } : node)));
                            }}
                        />
                    ) : null}

                    {drawingNode ? (
                        <CanvasLazyEditor
                            load={loadDrawingEditor}
                            errorTitle="绘图编辑器"
                            fallback={
                                <div className="fixed inset-0 z-[var(--z-toast)] grid place-items-center px-5" style={{ background: theme.canvas.background, color: theme.node.text }}>
                                    <WorkspaceState icon="loading" title={canvasT("videoCanvas.toast.loadingDrawingEditor", "正在加载绘图编辑器")} description={canvasT("videoCanvas.toast.preparingDrawing", "正在准备绘图画布。")} />
                                </div>
                            }
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
                    ) : null}

                    {activeScriptNode ? (
                        <CanvasLazyEditor
                            load={loadScriptEditor}
                            errorTitle="分镜编辑器"
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
                    ) : null}

                    {directorNodeId && activeDirectorScene ? (
                        <CanvasLazyEditor
                            load={loadDirectorWorkbench}
                            errorTitle="3D 导演台"
                            fallback={
                                <div className="fixed inset-0 z-[var(--z-toast)] grid place-items-center px-5" style={{ background: theme.canvas.background, color: theme.node.text }}>
                                    <WorkspaceState icon="loading" title={canvasT("videoCanvas.toast.loadingDirector", "正在加载 3D 导演台")} description={canvasT("videoCanvas.toast.preparingDirector", "准备场景、镜头与空间控制。")} />
                                </div>
                            }
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

                    <CanvasProjectAssetModal
                        open={projectAssetOpen}
                        detail={linkedProjectQuery.data}
                        initialCategory={projectAssetInitialCategory}
                        onClose={closeProjectAssets}
                        onInsert={(payloads) => handleProjectAssetsInsert(payloads, projectAssetInsertPosition)}
                    />
                    {codexCompactAgent && !assistantMounted ? (
                        <CanvasLazyEditor
                            load={loadLocalAgentPanel}
                            errorTitle="画布本地 Agent"
                            fallback={null}
                            headless
                            snapshot={agentSnapshot}
                            canUndoOps={canUndoAgentOps}
                            undoOpsCount={agentUndoCount}
                            onApplyOps={applyAgentOps}
                            onUndoOps={undoAgentOps}
                            autoConnect={codexAutoConnect}
                        />
                    ) : null}
        </>
    );
}
