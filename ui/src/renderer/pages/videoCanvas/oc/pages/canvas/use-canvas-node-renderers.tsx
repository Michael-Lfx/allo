import { useCallback, useRef } from "react";
import type { Dispatch, ReactNode, RefObject, SetStateAction } from "react";
import { CanvasConfigComposer } from "@oc/components/canvas/canvas-config-composer";
import { CanvasConfigNodePanel } from "@oc/components/canvas/canvas-config-node-panel";
import { CanvasNodePromptPanel } from "@oc/components/canvas/canvas-node-prompt-panel";
import { CanvasCharacterReferenceNodeContent } from "@oc/components/canvas/canvas-character-reference-node";
import { CanvasScriptNodeContent, storyboardMinNodeHeight } from "@oc/components/canvas/canvas-script-node";
import { CanvasDirectorNodePanel } from "@oc/components/canvas/director/canvas-director-node-panel";
import { CanvasStoryInputNodeContent, CanvasStyleNodeContent } from "@oc/components/canvas/canvas-short-drama-entry";
import { getInputSummary } from "@oc/lib/canvas/canvas-project-domain";
import { getNodeGenerationMode } from "@oc/lib/canvas/node-registry";
import { deriveStoryboardPipelineProgress } from "@oc/lib/canvas/canvas-storyboard-progress";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type CanvasWorkspaceMode, type StoryboardShotCount, type StoryboardShotDuration } from "@oc/types/canvas";
import type { CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import type { DirectorScene } from "@oc/types/director";
import type { useCanvasRenderModel } from "./use-canvas-render-model";
import type { useCanvasNodeEditor } from "./use-canvas-node-editor";
import type { useCanvasGeneration } from "./use-canvas-generation";
import type { useCanvasGenerationExecutor } from "./use-canvas-generation-executor";
import type { useCanvasConnectionController } from "./use-canvas-connection-controller";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";
import type { useCanvasStoryboard } from "./use-canvas-storyboard";
import type { useCanvasGenerationBatches } from "./use-canvas-generation-batches";
import type { useCanvasMediaTools } from "./use-canvas-media-tools";
import type { useCanvasDirector } from "./use-canvas-director";
import type { useCanvasShortDrama } from "./use-canvas-short-drama";

const EMPTY_RESOURCE_REFERENCES: CanvasResourceReference[] = [];

function visibleGenerationBatch(node: CanvasNodeData) {
    const batches = node.metadata?.generationBatches || [];
    for (let index = batches.length - 1; index >= 0; index -= 1) {
        if (batches[index].status === "queued" || batches[index].status === "running") return batches[index];
    }
    return batches.at(-1);
}

type CanvasNodeRenderersInput = {
    addScriptRow: ReturnType<typeof useCanvasStoryboard>["addScriptRow"];
    cancelSubmittedBatchItem: ReturnType<typeof useCanvasGenerationBatches>["cancelSubmittedBatchItem"];
    configInputsById: ReturnType<typeof useCanvasRenderModel>["configInputsById"];
    confirmStopGeneration: ReturnType<typeof useCanvasGeneration>["confirmStopGeneration"];
    createAndGenerateScriptVideos: ReturnType<typeof useCanvasStoryboard>["createAndGenerateScriptVideos"];
    createScriptActionBoards: ReturnType<typeof useCanvasStoryboard>["createScriptActionBoards"];
    createScriptImageNodes: ReturnType<typeof useCanvasStoryboard>["createScriptImageNodes"];
    createScriptVideoNodes: ReturnType<typeof useCanvasStoryboard>["createScriptVideoNodes"];
    directorScenes: DirectorScene[] | undefined;
    generateScriptImages: ReturnType<typeof useCanvasStoryboard>["generateScriptImages"];
    generateScriptRows: ReturnType<typeof useCanvasStoryboard>["generateScriptRows"];
    generateScriptVideos: ReturnType<typeof useCanvasStoryboard>["generateScriptVideos"];
    handleConfigNodeChange: ReturnType<typeof useCanvasNodeEditor>["handleConfigNodeChange"];
    handleConnectStart: ReturnType<typeof useCanvasConnectionController>["handleConnectStart"];
    handleGenerateNode: ReturnType<typeof useCanvasGenerationExecutor>;
    handleNodePromptChange: ReturnType<typeof useCanvasNodeEditor>["handleNodePromptChange"];
    handleNodeResize: ReturnType<typeof useCanvasNodeEditor>["handleNodeResize"];
    mentionReferencesByNodeId: ReturnType<typeof useCanvasRenderModel>["mentionReferencesByNodeId"];
    mergeVideosByIds: ReturnType<typeof useCanvasMediaTools>["mergeVideosByIds"];
    nodesRef: RefObject<CanvasNodeData[]>;
    connectionsRef: RefObject<CanvasConnection[]>;
    openDirectorWorkbench: ReturnType<typeof useCanvasDirector>["openDirectorWorkbench"];
    openStoryInput: ReturnType<typeof useCanvasShortDrama>["openStoryInput"];
    removeScriptRow: ReturnType<typeof useCanvasStoryboard>["removeScriptRow"];
    retryFailedBatchItems: ReturnType<typeof useCanvasGenerationBatches>["retryFailedBatchItems"];
    runningNodeId: string | null;
    skillMentionReferences: ReturnType<typeof useCanvasRenderModel>["skillMentionReferences"];
    stopRemainingBatchItems: ReturnType<typeof useCanvasGenerationBatches>["stopRemainingBatchItems"];
    updateScriptRow: ReturnType<typeof useCanvasStoryboard>["updateScriptRow"];
    workspaceMode: CanvasWorkspaceMode;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setNodeImageSettingsOpen: Dispatch<SetStateAction<boolean>>;
    setScriptEditorNodeId: Dispatch<SetStateAction<string | null>>;
    setScriptScrollTopById: Dispatch<SetStateAction<Record<string, number>>>;
    setStylePickerOpen: Dispatch<SetStateAction<boolean>>;
    setToolbarNodeId: Dispatch<SetStateAction<string | null>>;
};

export function useCanvasNodeRenderers(input: CanvasNodeRenderersInput) {
    const {
        addScriptRow,
        cancelSubmittedBatchItem,
        configInputsById,
        confirmStopGeneration,
        createAndGenerateScriptVideos,
        createScriptActionBoards,
        createScriptImageNodes,
        createScriptVideoNodes,
        directorScenes,
        generateScriptImages,
        generateScriptRows,
        generateScriptVideos,
        handleConfigNodeChange,
        handleConnectStart,
        handleGenerateNode,
        handleNodePromptChange,
        handleNodeResize,
        mentionReferencesByNodeId,
        mergeVideosByIds,
        nodesRef,
        connectionsRef,
        openDirectorWorkbench,
        openStoryInput,
        removeScriptRow,
        retryFailedBatchItems,
        runningNodeId,
        skillMentionReferences,
        stopRemainingBatchItems,
        updateScriptRow,
        workspaceMode,
        setDialogNodeId,
        setNodeImageSettingsOpen,
        setScriptEditorNodeId,
        setScriptScrollTopById,
        setStylePickerOpen,
        setToolbarNodeId,
    } = input;

    const renderCanvasNodePanel = useCallback(
        (panelNode: CanvasNodeData) => {
            if (panelNode.type === CanvasNodeType.Script || panelNode.type === CanvasNodeType.Drawing) return null;
            return panelNode.type === CanvasNodeType.Config ? (
                <CanvasConfigComposer
                    value={panelNode.metadata?.composerContent ?? panelNode.metadata?.prompt ?? ""}
                    inputs={configInputsById.get(panelNode.id) || []}
                    skillReferences={skillMentionReferences}
                    generationMode={getNodeGenerationMode(panelNode) ?? undefined}
                    metadata={panelNode.metadata}
                    workspaceMode={workspaceMode}
                    onChange={(composerContent) => handleConfigNodeChange(panelNode.id, { composerContent })}
                    onMetadataChange={(patch) => handleConfigNodeChange(panelNode.id, patch)}
                    onClose={() => setDialogNodeId(null)}
                />
            ) : (
                <CanvasNodePromptPanel
                    node={panelNode}
                    isRunning={runningNodeId === panelNode.id}
                    mentionReferences={mentionReferencesByNodeId.get(panelNode.id) || EMPTY_RESOURCE_REFERENCES}
                    onPromptChange={handleNodePromptChange}
                    onConfigChange={handleConfigNodeChange}
                    onGenerate={handleGenerateNode}
                    onStop={confirmStopGeneration}
                    workspaceMode={workspaceMode}
                    onImageSettingsOpenChange={(open) => {
                        setNodeImageSettingsOpen(open);
                        if (open) setToolbarNodeId(null);
                    }}
                />
            );
        },
        [configInputsById, confirmStopGeneration, handleConfigNodeChange, handleGenerateNode, handleNodePromptChange, mentionReferencesByNodeId, runningNodeId, skillMentionReferences, workspaceMode],
    );

    // 回调身份恒定：易变依赖经 ref 桶读取（渲染期同步赋值，子组件渲染时必读到最新提交值）。
    // 否则任一依赖变化都会让所有可见节点的 renderNodeContent prop 换新身份、整体击穿 CanvasNode.memo。
    const nodeContentInputsRef = useRef({ addScriptRow, cancelSubmittedBatchItem, configInputsById, confirmStopGeneration, createAndGenerateScriptVideos, createScriptActionBoards, createScriptImageNodes, createScriptVideoNodes, directorScenes, generateScriptImages, generateScriptRows, generateScriptVideos, handleConfigNodeChange, handleConnectStart, handleGenerateNode, handleNodeResize, mentionReferencesByNodeId, mergeVideosByIds, openDirectorWorkbench, openStoryInput, removeScriptRow, retryFailedBatchItems, runningNodeId, stopRemainingBatchItems, updateScriptRow, workspaceMode });
    nodeContentInputsRef.current = { addScriptRow, cancelSubmittedBatchItem, configInputsById, confirmStopGeneration, createAndGenerateScriptVideos, createScriptActionBoards, createScriptImageNodes, createScriptVideoNodes, directorScenes, generateScriptImages, generateScriptRows, generateScriptVideos, handleConfigNodeChange, handleConnectStart, handleGenerateNode, handleNodeResize, mentionReferencesByNodeId, mergeVideosByIds, openDirectorWorkbench, openStoryInput, removeScriptRow, retryFailedBatchItems, runningNodeId, stopRemainingBatchItems, updateScriptRow, workspaceMode };

    const renderCanvasNodeContent = useCallback(
        (contentNode: CanvasNodeData) => {
            const { addScriptRow, cancelSubmittedBatchItem, configInputsById, confirmStopGeneration, createAndGenerateScriptVideos, createScriptActionBoards, createScriptImageNodes, createScriptVideoNodes, directorScenes, generateScriptImages, generateScriptVideos, handleConfigNodeChange, handleConnectStart, handleGenerateNode, handleNodeResize, mentionReferencesByNodeId, mergeVideosByIds, openDirectorWorkbench, openStoryInput, removeScriptRow, retryFailedBatchItems, runningNodeId, stopRemainingBatchItems, updateScriptRow, workspaceMode } = nodeContentInputsRef.current;
            if (contentNode.metadata?.workflowKind === "character" && contentNode.metadata.characterAssetId) {
                return <CanvasCharacterReferenceNodeContent node={contentNode} />;
            }
            if (contentNode.metadata?.workflowKind === "styleboard") {
                return <CanvasStyleNodeContent node={contentNode} onChoose={() => setStylePickerOpen(true)} />;
            }
            if (contentNode.metadata?.workflowKind === "story_input") {
                return <CanvasStoryInputNodeContent node={contentNode} onEdit={() => openStoryInput(contentNode.id)} />;
            }
            if (contentNode.type === CanvasNodeType.Script) {
                const pipeline = deriveStoryboardPipelineProgress(contentNode, nodesRef.current, connectionsRef.current);
                const rowIds = pipeline.rows.map((item) => item.row.id);
                return (
                    <CanvasScriptNodeContent
                        node={contentNode}
                        batch={visibleGenerationBatch(contentNode)}
                        pipeline={pipeline}
                        mentionReferences={mentionReferencesByNodeId.get(contentNode.id) || EMPTY_RESOURCE_REFERENCES}
                        onOpen={() => setScriptEditorNodeId(contentNode.id)}
                        onCreateImageNodes={() => createScriptImageNodes(contentNode.id)}
                        onCreateVideoNodes={() => createScriptVideoNodes(contentNode.id)}
                        onGenerateImages={() => void generateScriptImages(contentNode.id, rowIds)}
                        onGenerateVideos={() => (contentNode.metadata?.storyboardVideoInputMode === "keyframe" ? void generateScriptVideos(contentNode.id, rowIds) : void createAndGenerateScriptVideos(contentNode.id, rowIds))}
                        onVideoInputModeChange={(storyboardVideoInputMode) => handleConfigNodeChange(contentNode.id, { storyboardVideoInputMode })}
                        onMergeVideos={() => void mergeVideosByIds(pipeline.successfulVideoNodeIds)}
                        onCreateActionBoards={() => void createScriptActionBoards(contentNode.id)}
                        onRetryBatch={(batchId) => retryFailedBatchItems(contentNode.id, batchId)}
                        onRetryBatchItem={(batchId, itemId) => retryFailedBatchItems(contentNode.id, batchId, itemId)}
                        onStopBatch={(batchId) => stopRemainingBatchItems(contentNode.id, batchId)}
                        onCancelBatchItem={(batchId, itemId) => cancelSubmittedBatchItem(contentNode.id, batchId, itemId)}
                        onAddRow={() => addScriptRow(contentNode.id)}
                        onRemoveRow={(rowId) => removeScriptRow(contentNode.id, rowId)}
                        onUpdateRow={(rowId, patch) => updateScriptRow(contentNode.id, rowId, patch)}
                        onPromptChange={(composerContent) => handleConfigNodeChange(contentNode.id, { composerContent })}
                        onGenerateScript={(prompt) => void generateScriptRows(contentNode.id, prompt)}
                        onModelChange={(model) => handleConfigNodeChange(contentNode.id, { model })}
                        onShotDurationChange={(duration: StoryboardShotDuration) => handleConfigNodeChange(contentNode.id, { storyboardShotDuration: duration })}
                        onShotCountChange={(count: StoryboardShotCount) => handleConfigNodeChange(contentNode.id, { storyboardShotCount: count })}
                        workspaceMode={workspaceMode}
                        onComposerHeightChange={(height) => {
                            if (contentNode.metadata?.storyboardComposerHeight === height) return;
                            handleConfigNodeChange(contentNode.id, { storyboardComposerHeight: height });
                            const minHeight = storyboardMinNodeHeight(height);
                            if (contentNode.height < minHeight) handleNodeResize(contentNode.id, contentNode.width, minHeight);
                        }}
                        onConnectStart={(event, rowId, handleType) => handleConnectStart(event, contentNode.id, handleType, rowId === "context" ? "storyboard:context" : `row:${rowId}`)}
                        onScrollTopChange={(scrollTop) => setScriptScrollTopById((current) => (current[contentNode.id] === scrollTop ? current : { ...current, [contentNode.id]: scrollTop }))}
                    />
                );
            }
            if (contentNode.metadata?.directorSceneId) {
                return (
                    <CanvasDirectorNodePanel
                        node={contentNode}
                        scene={directorScenes?.find((scene) => scene.id === contentNode.metadata?.directorSceneId) || null}
                        readNodeContent={(nodeId) => nodesRef.current.find((item) => item.id === nodeId)?.metadata?.content}
                        onOpen={() => openDirectorWorkbench(contentNode.id)}
                    />
                );
            }
            return (
                <CanvasConfigNodePanel
                    node={contentNode}
                    isRunning={runningNodeId === contentNode.id}
                    inputSummary={getInputSummary(configInputsById.get(contentNode.id) || [])}
                    onConfigChange={handleConfigNodeChange}
                    onComposerToggle={() => setDialogNodeId((current) => (current === contentNode.id ? null : contentNode.id))}
                    onStop={confirmStopGeneration}
                    onGenerate={(nodeId) => {
                        const target = nodesRef.current.find((item) => item.id === nodeId);
                        void handleGenerateNode(nodeId, (target && getNodeGenerationMode(target)) || target?.metadata?.generationMode || "image", target?.metadata?.composerContent ?? target?.metadata?.prompt ?? "");
                    }}
                    workspaceMode={workspaceMode}
                />
            );
        },
        [],
    );

    return { renderCanvasNodePanel, renderCanvasNodeContent };
}
