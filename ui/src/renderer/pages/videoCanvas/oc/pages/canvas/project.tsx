import { stampCanvasNodeChanges } from "@oc/lib/canvas/canvas-node-timestamps";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { useQuery } from "@tanstack/react-query";
import { useParams, useSearchParams } from "react-router-dom";
import { useConfigStore, useEffectiveConfig } from "@oc/stores/use-config-store";
import { canvasThemes, type CanvasBackgroundMode } from "@oc/lib/canvas-theme";
import { readCanvasMediaPerformanceMode } from "@oc/lib/canvas/canvas-performance-mode";
import { summarizeCanvasContext } from "@oc/lib/canvas/canvas-context-summary";
import { refreshCanvasCharacterReferenceNodes } from "@oc/lib/canvas/canvas-character-reference";
import { useAssetStore } from "@oc/stores/use-asset-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import { App } from "antd";
import { CanvasProjectSidebar } from "@oc/components/canvas/canvas-project-sidebar";
import { getProject } from "@oc/services/api/projects";
import { useFocusMode } from "@oc/hooks/use-focus-mode";
import { useCanvasAgentStore } from "@oc/stores/canvas/use-canvas-agent-store";
import { useCanvasInteractionStore } from "@oc/stores/canvas/use-canvas-interaction-store";
import { batchSourceRestriction } from "@oc/lib/canvas/canvas-batch-connection";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { readCanvasWorkspaceMode } from "@oc/lib/canvas/canvas-project-domain";
import { CanvasRefreshShell } from "./canvas-refresh-shell";
import { useCanvasConnectionController } from "./use-canvas-connection-controller";
import { useCanvasAgentOperations } from "./use-canvas-agent-operations";
import { useCanvasAssistantVisibility } from "./use-canvas-assistant-visibility";
import { useCanvasActiveTasks } from "./use-canvas-active-tasks";
import { useCanvasStyleWorkflow } from "./use-canvas-style-workflow";
import { useCanvasDirector } from "./use-canvas-director";
import { useCanvasGeneration } from "./use-canvas-generation";
import { useCanvasGenerationBatches } from "./use-canvas-generation-batches";
import { useCanvasGenerationExecutor } from "./use-canvas-generation-executor";
import { useCanvasGenerationRetry } from "./use-canvas-generation-retry";
import { useCanvasHistory } from "./use-canvas-history";
import { useCanvasKeyboard } from "./use-canvas-keyboard";
import { useCanvasMediaTools } from "./use-canvas-media-tools";
import { useCanvasNodeEditor } from "./use-canvas-node-editor";
import { useCanvasNodeOperations } from "./use-canvas-node-operations";
import { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";
import { useCanvasProjectShare } from "./use-canvas-project-share";
import { useCanvasRenderModel } from "./use-canvas-render-model";
import { useCanvasSelectionController } from "./use-canvas-selection-controller";
import { useCanvasShortDrama } from "./use-canvas-short-drama";
import { useCanvasStoryboard } from "./use-canvas-storyboard";
import { useCanvasUpload } from "./use-canvas-upload";
import { useCanvasViewportController } from "./use-canvas-viewport-controller";
import { useTranslation } from "react-i18next";
import { useCanvasDialogState, useCanvasAssistantPanelWidth } from "./use-canvas-panel-state";
import { useCanvasNodeDeletion, useCanvasClearCanvas } from "./use-canvas-node-deletion";
import { useCanvasNodeActions } from "./use-canvas-node-actions";
import { useCanvasToolbarVisibility } from "./use-canvas-toolbar-visibility";
import { useCanvasNodeRenderers } from "./use-canvas-node-renderers";
import type { CanvasNodeGenerationMode } from "@oc/components/canvas/canvas-node-prompt-panel";
import { useCanvasClipboardActions } from "./use-canvas-clipboard";
import { useCanvasContextMenuActions } from "./use-canvas-context-menu";
import { useCanvasTitleEditing } from "./use-canvas-title-editing";
import { useCanvasNodeGenerationActions } from "./use-canvas-node-generation";
import { useCanvasContainerSize, useCanvasStylePresetSync } from "./use-canvas-sync-effects";
import { useCanvasChromeEffects } from "./use-canvas-chrome-effects";
import { CanvasProjectTopChrome } from "./canvas-project-top-chrome";
import { CanvasProjectStage } from "./canvas-project-stage";
import { CanvasProjectAssistantColumn } from "./canvas-project-assistant-column";
import { CanvasProjectOverlays } from "./canvas-project-overlays";
import { CanvasProjectCanvasChrome } from "./canvas-project-chrome";
import { CanvasProjectDialogs } from "./canvas-project-dialogs";
import { CanvasProjectEmptyState } from "./canvas-project-empty-state";
import {
    CanvasNodeType,
    type CanvasAssistantSession,
    type CanvasConnection,
    type CanvasNodeData,
    type CanvasMediaPerformanceMode,
    type CanvasWorkspaceMode,
    type CanvasToolMode,
    type ContextMenuState,
    type ViewportTransform,
} from "@oc/types/canvas";

type CanvasPageProps = {
    modelCatalogReady: boolean;
};

export default function CanvasPage({ modelCatalogReady }: CanvasPageProps) {
    const [mounted, setMounted] = useState(false);

    useEffect(() => {
        setMounted(true);
    }, []);

    if (!mounted) return <CanvasRefreshShell />;

    return <InfiniteCanvasPage modelCatalogReady={modelCatalogReady} />;
}

function InfiniteCanvasPage({ modelCatalogReady }: CanvasPageProps) {
    useTranslation();
    const { message } = App.useApp();
    const params = useParams<{ id: string }>();
    const [searchParams] = useSearchParams();
    const projectId = params.id || "";
    const localAgentConnected = useCanvasAgentStore((state) => state.connected);
    const localAgentActivity = useCanvasAgentStore((state) => state.activity);
    const localAgentEnabled = useCanvasAgentStore((state) => state.enabled);
    const containerRef = useRef<HTMLDivElement>(null);
    const didInitialCenterRef = useRef(false);
    const toolbarHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const setHoveredNodeId = useCanvasInteractionStore((state) => state.setHoveredNodeId);
    const setToolbarNodeId = useCanvasInteractionStore((state) => state.setToolbarNodeId);
    const interactionSetters = { setHoveredNodeId, setToolbarNodeId };

    const config = useConfigStore((state) => state.config);
    const effectiveConfig = useEffectiveConfig();
    const isAiConfigReady = useConfigStore((state) => state.isAiConfigReady);
    const assets = useAssetStore((state) => state.assets);
    const cleanupAssetImages = useAssetStore((state) => state.cleanupImages);
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const user = useUserStore((state) => state.user);
    const defaultDrawingEngine = useUserStore((state) => state.drawingEngine.defaultEngine);
    const shortDramaEnabled = useUserStore((state) => state.features.shortDramaEnabled);
    const nodesRef = useRef<CanvasNodeData[]>([]);
    const [nodes, setNodesState] = useState<CanvasNodeData[]>([]);
    const setNodes = useCallback<Dispatch<SetStateAction<CanvasNodeData[]>>>((value) => {
        if (typeof value === "function") {
            setNodesState((current) => {
                const next = stampCanvasNodeChanges(current, value(current));
                nodesRef.current = next;
                return next;
            });
            return;
        }
        const next = stampCanvasNodeChanges(nodesRef.current, value);
        nodesRef.current = next;
        setNodesState(next);
    }, []);
    const [connections, setConnections] = useState<CanvasConnection[]>([]);
    const [chatSessions, setChatSessions] = useState<CanvasAssistantSession[]>([]);
    const [activeChatId, setActiveChatId] = useState<string | null>(null);
    const [viewport, setViewport] = useState<ViewportTransform>({ x: 0, y: 0, k: 1 });
    const [size, setSize] = useState({ width: 1200, height: 720 });
    const [selectedNodeIds, setSelectedNodeIds] = useState<Set<string>>(new Set());
    const [selectedConnectionId, setSelectedConnectionId] = useState<string | null>(null);
    const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
    const [isMiniMapOpen, setIsMiniMapOpen] = useState(false);
    const [backgroundMode, setBackgroundMode] = useState<CanvasBackgroundMode>("lines");
    const [showImageInfo, setShowImageInfo] = useState(false);
    const [canvasTool, setCanvasTool] = useState<CanvasToolMode>("move");
    const [mediaPerformanceMode, setMediaPerformanceMode] = useState<CanvasMediaPerformanceMode>(readCanvasMediaPerformanceMode);
    const [projectLoaded, setProjectLoaded] = useState(false);
    const [workspaceMode, setWorkspaceMode] = useState<CanvasWorkspaceMode>(readCanvasWorkspaceMode);
    const dialogState = useCanvasDialogState();
    const {
        setNodeSearchOpen,
        nodeImageSettingsOpen,
        setNodeImageSettingsOpen,
        dialogNodeId,
        setDialogNodeId,
        textEditorNodeId,
        characterReferenceNodeId,
        drawingNodeId,
        setDrawingNodeId,
        setStylePickerOpen,
        setInfoNodeId,
        scriptScrollTopById,
        directorNodeId,
        setDirectorNodeId,
        setShortcutRequestNonce,
        setCinematicAgentEntry,
        openProjectAssets,
    } = dialogState;
    const codexAutoConnect = ["new", "recent", "choose"].includes(searchParams.get("mode") || "");
    const codexCompactAgent = codexAutoConnect && searchParams.has("agentUrl");
    const { assistantWidth, setAssistantWidth } = useCanvasAssistantPanelWidth();
    const { agentMode, assistantClosing, assistantMounted, assistantOpen, closeAgent, openAgent, setAgentMode } = useCanvasAssistantVisibility();
    const assistant = { agentMode, assistantClosing, assistantMounted, assistantOpen, closeAgent, openAgent, setAgentMode };
    const { tasks: activeTasks } = useCanvasActiveTasks(projectId, projectLoaded);
    const { focusMode, enterFocusMode, exitFocusMode, toggleFocusMode } = useFocusMode();
    const [focusDockRevealed, setFocusDockRevealed] = useState(false);

    useCanvasChromeEffects({ projectId, didInitialCenterRef, workspaceMode, mediaPerformanceMode, focusMode, dialogNodeId, searchParams, projectLoaded, openAgent, setAgentMode, closeAgent, setNodeSearchOpen, setIsMiniMapOpen, setFocusDockRevealed, setNodeImageSettingsOpen });

    const connectionsRef = useRef(connections);
    const selectedNodeIdsRef = useRef(selectedNodeIds);
    const viewportRef = useRef(viewport);
    const generateNodeRef = useRef<((nodeId: string, mode: CanvasNodeGenerationMode, prompt: string) => Promise<void>) | null>(null);

    // 画布核心 setter/ref 束：多数子 hook 与组件共用同一组变更入口，集中声明避免逐处重复。
    const canvasSetters = { setNodes, setConnections, setSelectedNodeIds, setSelectedConnectionId, setContextMenu, setDialogNodeId };
    const canvasRefs = { nodesRef, connectionsRef, selectedNodeIdsRef, viewportRef };

    const { getHistoryCleanupContext, historyPausedRef, historyState, redoCanvas, resetHistory, undoCanvas } = useCanvasHistory({
        ...canvasSetters,
        projectLoaded,
        nodes,
        connections,
        chatSessions,
        activeChatId,
        backgroundMode,
        showImageInfo,
        setChatSessions,
        setActiveChatId,
        setBackgroundMode,
        setShowImageInfo,
    });
    const cleanupCanvasFiles = useCallback(
        (extra?: unknown) => {
            cleanupAssetImages({ extra, ...getHistoryCleanupContext() });
        },
        [cleanupAssetImages, getHistoryCleanupContext],
    );

    const { addedSkills, clearCanvasFiles, createAndOpenProject, currentProject, deleteCurrentProject, renameCurrentProject, saveCanvasProject, updateProject } = useCanvasProjectLifecycle({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        projectLoaded,
        nodes,
        connections,
        chatSessions,
        activeChatId,
        backgroundMode,
        showImageInfo,
        viewport,
        historyPausedRef,
        setChatSessions,
        setActiveChatId,
        setBackgroundMode,
        setShowImageInfo,
        setViewport,
        setProjectLoaded,
        resetHistory,
        cleanupAssetImages,
        cleanupCanvasFiles,
    });
    const projectShare = useCanvasProjectShare(currentProject);
    const linkedProjectId = shortDramaEnabled ? currentProject?.projectId || "" : "";
    const linkedProjectQuery = useQuery({ queryKey: ["project", linkedProjectId], queryFn: () => getProject(linkedProjectId), enabled: Boolean(linkedProjectId) });
    useEffect(() => {
        if (!projectLoaded || !linkedProjectQuery.data) return;
        setNodes((current) => refreshCanvasCharacterReferenceNodes(current, linkedProjectQuery.data.assets));
    }, [linkedProjectQuery.data, projectLoaded, setNodes]);
    const canvasContext = useMemo(() => summarizeCanvasContext(nodes, selectedNodeIds, linkedProjectQuery.data?.units), [linkedProjectQuery.data?.units, nodes, selectedNodeIds]);

    const { bindGenerationTask, cancelNodeTask, confirmStopGeneration, finishGenerationRequest, openNodeTaskDetails, reloadCanvasNodeResource, runningNodeId, setRunningNodeId, setTaskDetail, startGenerationRequest, taskDetail, taskDetailLoading, taskDetailLogs } =
        useCanvasGeneration({ projectId, domainProjectId: linkedProjectId, projectLoaded, nodes, nodesRef, setNodes });


    useLayoutEffect(() => {
        nodesRef.current = nodes;
        connectionsRef.current = connections;
        selectedNodeIdsRef.current = selectedNodeIds;
        viewportRef.current = viewport;
    }, [nodes, connections, selectedNodeIds, viewport]);

    useCanvasContainerSize({ containerRef, projectLoaded, viewportRef, setViewport, setSize, didInitialCenterRef });

    const {
        fitCanvasContent,
        fitCanvasSelection,
        focusCanvasImageNode,
        focusCanvasNode,
        getCanvasCenter,
        handleCanvasDoubleClick,
        handleViewportChange,
        handleViewportPreviewChange,
        previewViewport,
        resetViewport,
        screenToCanvas,
        setZoomScale,
        zoomCanvasIn,
        zoomCanvasOut,
        zoomToActualSize,
    } = useCanvasViewportController({
        ...interactionSetters,
        ...canvasSetters,
        ...canvasRefs,
        containerRef,
        size,
        setViewport,
    });
    const viewportActions = { focusCanvasNode, focusCanvasImageNode, screenToCanvas, previewViewport, setZoomScale };

    useCanvasStylePresetSync({ projectLoaded, linkedProjectQuery, nodesRef, setNodes, getCanvasCenter });

    const {
        assetPickerOpen,
        closeAssetPicker,
        createMediaAssetNode,
        fileDropActive,
        handleAssetInsert,
        handleDrop,
        handleFileDragEnter,
        handleFileDragLeave,
        handleFileDragOver,
        handleImageInputChange,
        handleProjectAssetsInsert,
        handleProjectChapterInsert,
        handleUploadRequest,
        imageInputRef,
        openAssetsAtPosition,
        pasteAssistantImage,
        pasteSystemClipboard,
        startUploadStatus,
        uploadStatus,
    } = useCanvasUpload({
        ...canvasSetters,
        ...canvasRefs,
        canvasId: projectId,
        domainProjectId: linkedProjectId,
        getCanvasCenter,
        screenToCanvas,


    });
    const {
        angleNodeId,
        emotionNodeId,
        annotationNodeId,
        createImageReversePromptNodes,
        openPortraitTextureEditor,
        cropImageNode,
        cropNodeId,
        closeFrameDialog,
        extractVideoFrames,
        extractingVideoFrameNodeId,
        frameDialogNodeId,
        openVideoFrameExtractor,
        generateAngleNode,
        generateEmotionNode,
        maskEditImageNode,
        maskEditNodeId,
        mergeSelectedVideos,
        mergeVideosByIds,
        mergeVideoProgress,
        saveAnnotatedImageNode,
        setAngleNodeId,
        setEmotionNodeId,
        setAnnotationNodeId,
        setCropNodeId,
        setMaskEditNodeId,
        setSplitNodeId,
        setUpscaleNodeId,
        splitImageNode,
        splitNodeId,
        upscaleImageNode,
        upscaleNodeId,
    } = useCanvasMediaTools({
        ...interactionSetters,
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        setRunningNodeId,
        startUploadStatus,
        startGenerationRequest,
        finishGenerationRequest,
        bindGenerationTask,
    });
    const mediaSetters = { setAngleNodeId, setAnnotationNodeId, setCropNodeId, setEmotionNodeId, setMaskEditNodeId, setSplitNodeId, setUpscaleNodeId };
    const mediaActions = { generateAngleNode, generateEmotionNode, openPortraitTextureEditor, createImageReversePromptNodes, openVideoFrameExtractor, extractingVideoFrameNodeId, mergeSelectedVideos, mergeVideoProgress, mergeVideosByIds };
    const mediaDialogs = { cropImageNode, saveAnnotatedImageNode, maskEditImageNode, splitImageNode, upscaleImageNode, extractVideoFrames, closeFrameDialog, frameDialogNodeId };
    const { handleNodesDeleted } = useCanvasNodeDeletion({
        ...interactionSetters,
        ...mediaSetters,
        ...dialogState,
        setRunningNodeId,
        setContextMenu,
        chatSessions,
        cleanupCanvasFiles,
        projectId,
        message,
    });
    const {
        alignSelectedNodes,
        arrangeSelectedNodes,
        copyNodesToClipboard,
        copySelectedNodes,
        createFolder,
        createNode,
        createReferenceGroup,
        createStoryboardGroup,
        deleteConnection,
        deleteNodes,
        duplicateNode,
        hasCopiedNodes,
        setTvCoverNode,
        pasteCopiedNodes,
        restoreCopiedNodesFromText,
        releaseCopiedNodesPastePriority,
        setPrimaryVersion,
        shouldPreferCopiedNodes,
        toggleNodeLocked,
    } = useCanvasNodeOperations({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        defaultDrawingEngine,
        getCanvasCenter,
        onNodesDeleted: handleNodesDeleted,
    });
    const {
        cancelPendingConnectionCreate,
        closeConnectionCreateMenu,
        createConnectedNode,
        handleConnectStart,
        handleBatchConnectionTargetClick,
        batchConnectionPreview,
        beginBatchConnectionMode,
        startBatchConnection,
        pendingConnectionCreate,
    } = useCanvasConnectionController({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        defaultDrawingEngine,
        scriptScrollTopById,
        screenToCanvas,
        setDrawingNodeId,
    });
    const batchSourceNodeIds = useMemo(() => nodes
        .filter((node) => selectedNodeIds.has(node.id) && !batchSourceRestriction(node))
        .map((node) => node.id), [nodes, selectedNodeIds]);

    const {
        handleCanvasSelectionStart,
        handleNodeInteractionStart,
        handleSelectedNodeClick,
        handleCanvasDeselect,
        openTextNodeEditor,
        openDrawingNode,
        openCanvasNodeTaskDetails,
        openCanvasNodeVersions,
        viewCanvasNodeImage,
        handleReplaceMedia,
        locateProjectStyleNode,
    } = useCanvasNodeActions({
        ...interactionSetters,
        ...dialogState,
        setSelectedNodeIds,
        setSelectedConnectionId,
        openNodeTaskDetails,
        handleUploadRequest,
        nodesRef,
        focusCanvasNode,
        message,
        setContextMenu,
    });
    const { cancelSelectionBox, deselectCanvas, handleCanvasMouseDown, handleNodeMouseDown, nodeDraggingRef, selectionBoundsElementRef } = useCanvasSelectionController({
        ...canvasSetters,
        ...canvasRefs,
        containerRef,
        historyPausedRef,
        screenToCanvas,
        cancelPendingConnectionCreate,
        onCanvasSelectionStart: handleCanvasSelectionStart,
        onNodeInteractionStart: handleNodeInteractionStart,
        onNodeClick: handleSelectedNodeClick,
        onBatchConnectionTarget: handleBatchConnectionTargetClick,
        onDeselect: handleCanvasDeselect,
        onSelectionBoxEnd: () => setCanvasTool((tool) => (tool === "box-select" ? "move" : tool)),
    });
    const { keepNodeToolbar, hideNodeToolbar, handleCanvasNodeHoverStart, handleCanvasNodeHoverEnd } = useCanvasToolbarVisibility({
        nodeDraggingRef,
        nodeImageSettingsOpen,
        setHoveredNodeId,
        setToolbarNodeId,
    });
    const historyActions = { historyState, undoCanvas, redoCanvas };

    const {
        collapsingBatchIds,
        downloadNodeImage,
        handleConfigNodeChange,
        handleFontSizeChange,
        handleNodeContentChange,
        handleNodePromptChange,
        handleNodeResize,
        handleNodeTitleChange,
        openingBatchIds,
        saveNodeAsset,
        setBatchPrimary,
        toggleBatchExpanded,
        toggleFrameCollapsed,
        toggleNodeFreeResize,
    } = useCanvasNodeEditor({
        ...interactionSetters,
        ...canvasSetters,
        ...canvasRefs,
        canvasId: projectId,
        domainProjectId: linkedProjectId,
        canvasTitle: currentProject?.title || canvasT("videoCanvas.chrome.untitled", "未命名画布"),
        onAssetSaved: () => openAssetsAtPosition(),
    });
    const editorActions = { handleConfigNodeChange, handleFontSizeChange, handleNodePromptChange, handleNodeResize, downloadNodeImage, saveNodeAsset, toggleNodeFreeResize, toggleNodeLocked };

    const renderModel = useCanvasRenderModel({
        ...dialogState,
        nodes,
        connections,
        assets,
        viewport,
        viewportSize: size,
        mediaPerformanceMode,
        selectedNodeIds,
        collapsingBatchIds,
        addedSkills,
        directorScenes: currentProject?.directorScenes,
        cropNodeId,
        maskEditNodeId,
        annotationNodeId,
        splitNodeId,
        upscaleNodeId,
        angleNodeId,
        emotionNodeId,
        contextMenu,
    });
    const { activeStylePresetId, configInputsById, mentionReferencesByNodeId, nodeById, skillMentionReferences } = renderModel;
    const dialogNode = dialogNodeId ? nodeById.get(dialogNodeId) || null : null;
    const textEditorNode = textEditorNodeId ? nodeById.get(textEditorNodeId) || null : null;
    const characterReferenceNode = characterReferenceNodeId ? nodeById.get(characterReferenceNodeId) || null : null;
    const drawingNode = drawingNodeId ? nodeById.get(drawingNodeId) || null : null;
    const pendingConnectionSourceNode = pendingConnectionCreate?.connection.handleType === "source" ? nodeById.get(pendingConnectionCreate.connection.nodeId) : null;
    const canCreateDrawingFromConnection = !pendingConnectionCreate?.batchSourceNodeIds?.length && pendingConnectionSourceNode?.type === CanvasNodeType.Image && Boolean(pendingConnectionSourceNode.metadata?.content);

    const { agentSnapshot, agentUndoCount, applyAgentOps, canUndoAgentOps, dismissLastAgentChange, lastAgentChange, undoAgentOps, viewLastAgentChange } = useCanvasAgentOperations({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        domainProjectId: currentProject?.projectId,
        projectTitle: currentProject?.title || canvasT("videoCanvas.chrome.untitled", "未命名画布"),
        nodes,
        connections,
        selectedNodeIds,
        viewport,
        generateNodeRef,
        setViewport,
        focusSelection: fitCanvasSelection,
    });
    const { selectCanvasStyle } = useCanvasStyleWorkflow({
        ...canvasSetters,
        ...canvasRefs,
        getCanvasCenter,
        setStylePickerOpen,
    });
    const { applyDirectorOutput, createDirectorShot, openDirectorWorkbench, saveDirectorScene } = useCanvasDirector({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        directorNodeId,
        directorScenes: currentProject?.directorScenes || [],
        getCanvasCenter,
        setDirectorNodeId,
        updateProject,
    });
    const directorActions = { createDirectorShot, openDirectorWorkbench, saveDirectorScene, applyDirectorOutput };

    const {
        activateStep: activateShortDramaStep,
        createPipeline: createShortDramaPipeline,
        guideCollapsed: shortDramaGuideCollapsed,
        openStoryInput,
        progress: shortDramaProgress,
        setGuideCollapsed: setShortDramaGuideCollapsed,
        skipGuide: skipShortDramaGuide,
    } = useCanvasShortDrama({
        ...canvasSetters,
        ...canvasRefs,
        nodes,
        connections,
        getCanvasCenter,
        setStylePickerOpen,
        fitCanvasSelection,
        focusCanvasNode,
        openTextEditor: openTextNodeEditor,
    });
    const shortDramaActions = { skipShortDramaGuide, activateShortDramaStep, openStoryInput, createShortDramaPipeline };

    const shortDramaGuide = shortDramaEnabled && !currentProject?.projectId && shortDramaProgress.active ? { progress: shortDramaProgress, collapsed: shortDramaGuideCollapsed, onToggle: () => setShortDramaGuideCollapsed((value) => !value) } : undefined;

    const { clearCanvas } = useCanvasClearCanvas({
        ...mediaSetters,
        ...dialogState,
        nodesRef,
        projectId,
        message,
        setNodes,
        setConnections,
        setRunningNodeId,
        deselectCanvas,
        clearCanvasFiles,
    });
    useCanvasKeyboard({
        ...historyActions,
        ...mediaSetters,
        ...canvasSetters,
        ...canvasRefs,
        selectedConnectionId,
        setShortcutRequestNonce,
        setInfoNodeId,
        saveCanvasProject,
        zoomToActualSize,
        fitCanvasContent,
        fitCanvasSelection,
        cancelSelectionBox,
        copySelectedNodes,
        pasteCopiedNodes,
        restoreCopiedNodesFromText,
        shouldPreferCopiedNodes,
        pasteSystemClipboard,
        deleteNodes,
        deleteConnection,
        deselectCanvas,
        zoomCanvasIn,
        zoomCanvasOut,
        focusMode,
        exitFocusMode,
        toggleFocusMode,
        onOpenSearch: () => setNodeSearchOpen(true),
        beginBatchConnection: () => beginBatchConnectionMode(Array.from(selectedNodeIdsRef.current)),
    });
    const handleAssistantSessionsChange = useCallback((sessions: CanvasAssistantSession[], activeId: string | null) => {
        setChatSessions(sessions);
        setActiveChatId(activeId);
    }, []);

    const { titleEditing, setTitleEditing, titleDraft, setTitleDraft, startTitleEditing, finishTitleEditing } = useCanvasTitleEditing({
        currentProject,
        renameCurrentProject,
    });
    const titleEditingState = { titleDraft, setTitleDraft, titleEditing, setTitleEditing, startTitleEditing, finishTitleEditing };
    const agentOps = { agentSnapshot, agentUndoCount, applyAgentOps, canUndoAgentOps, dismissLastAgentChange, lastAgentChange, undoAgentOps, viewLastAgentChange };

    const { pasteAtPosition, copyNodeContentToClipboard, copyNodeMediaUrlToClipboard } = useCanvasClipboardActions({
        message,
        shouldPreferCopiedNodes,
        pasteCopiedNodes,
        pasteSystemClipboard,
        releaseCopiedNodesPastePriority,
    });

    const { handleCanvasContextMenu, handleNodeContextMenu, handleConnectionSelect, handleConnectionContextMenu } = useCanvasContextMenuActions({
        closeConnectionCreateMenu,
        screenToCanvas,
        setContextMenu,
        setDialogNodeId,
        setHoveredNodeId,
        setToolbarNodeId,
        setSelectedConnectionId,
        setSelectedNodeIds,
    });
    const canvasInteraction = { handleViewportChange, handleViewportPreviewChange, handleCanvasMouseDown, handleCanvasDoubleClick, deselectCanvas, handleCanvasContextMenu, handleDrop, handleFileDragEnter, handleFileDragLeave, handleFileDragOver, resetViewport, exitFocusMode, zoomCanvasIn, zoomCanvasOut };

    const handleGenerateNode = useCanvasGenerationExecutor({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        domainProjectId: currentProject?.projectId,
        addedSkills,
        setRunningNodeId,
        startGenerationRequest,
        finishGenerationRequest,
        bindGenerationTask,
    });
    useEffect(() => {
        generateNodeRef.current = handleGenerateNode;
    }, [handleGenerateNode]);

    // Ordinary "视频生成" home mode: one-shot auto-start on the seeded config node.
    const homeAutoGenerateTriedRef = useRef(false);
    useEffect(() => {
        if (!projectLoaded || homeAutoGenerateTriedRef.current) return;
        const creative = currentProject?.alloCreative;
        const homeLaunch =
            creative && typeof creative === 'object' && 'homeLaunch' in creative
                ? (creative as { homeLaunch?: Record<string, unknown> }).homeLaunch
                : undefined;
        if (!homeLaunch || homeLaunch.autoGenerate !== true) return;
        const configNode = nodes.find(
            (node) =>
                node.type === CanvasNodeType.Config &&
                (node.metadata?.status === 'idle' || !node.metadata?.status)
        );
        if (!configNode) return;
        homeAutoGenerateTriedRef.current = true;
        updateProject(projectId, {
            alloCreative: {
                ...(typeof creative === 'object' && creative ? creative : {}),
                homeLaunch: { ...homeLaunch, autoGenerate: false },
            },
        });
        const prompt =
            configNode.metadata?.prompt?.trim() ||
            configNode.metadata?.composerContent?.trim() ||
            '';
        void handleGenerateNode(configNode.id, 'video', prompt);
    }, [
        projectLoaded,
        nodes,
        currentProject?.alloCreative,
        handleGenerateNode,
        updateProject,
        projectId,
    ]);

    const { cancelSubmittedBatchItem, enqueueGenerationBatch, retryFailedBatchItems, stopRemainingBatchItems } = useCanvasGenerationBatches({
        projectId,
        projectLoaded,
        modelCatalogReady,
        nodes,
        ...canvasRefs,
        setNodes,
        handleGenerateNode,
    });
    const batchActions = { cancelSubmittedBatchItem, retryFailedBatchItems, stopRemainingBatchItems };

    const { addScriptRow, createAndGenerateScriptVideos, createScriptActionBoards, createScriptImageNodes, createScriptVideoNodes, generateScriptImages, generateScriptRows, generateScriptVideos, removeScriptRow, replaceScriptRows, updateScriptRow } =
        useCanvasStoryboard({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        enqueueGenerationBatch,
    });
    const storyboardActions = { addScriptRow, removeScriptRow, updateScriptRow, replaceScriptRows, generateScriptImages, generateScriptRows, generateScriptVideos, createAndGenerateScriptVideos, createScriptActionBoards, createScriptImageNodes, createScriptVideoNodes, enqueueGenerationBatch };

    const handleRetryNode = useCanvasGenerationRetry({
        ...canvasSetters,
        ...canvasRefs,
        projectId,
        domainProjectId: currentProject?.projectId,
        addedSkills,
        setRunningNodeId,
        startGenerationRequest,
        finishGenerationRequest,
        bindGenerationTask,
    });
    const { generateImageFromTextNode, retryCanvasNode, editCanvasDirector } = useCanvasNodeGenerationActions({
        effectiveConfig,
        message,
        nodesRef,
        connectionsRef,
        setNodes,
        setConnections,
        setSelectedNodeIds,
        setSelectedConnectionId,
        setDialogNodeId,
        generateScriptRows,
        handleRetryNode,
        openDirectorWorkbench,
    });
    const worldLayerHandlers = {
        onConnectionSelect: handleConnectionSelect,
        onConnectionContextMenu: handleConnectionContextMenu,
        onNodeMouseDown: handleNodeMouseDown,
        onNodeHoverStart: handleCanvasNodeHoverStart,
        onNodeHoverEnd: handleCanvasNodeHoverEnd,
        onConnectStart: handleConnectStart,
        onNodeResize: handleNodeResize,
        onToggleFrame: toggleFrameCollapsed,
        onNodeTitleChange: handleNodeTitleChange,
        onNodeContextMenu: handleNodeContextMenu,
        onNodeContentChange: handleNodeContentChange,
        onToggleBatch: toggleBatchExpanded,
        onSetBatchPrimary: setBatchPrimary,
        onRetry: retryCanvasNode,
        onReloadResource: (node: CanvasNodeData) => { void reloadCanvasNodeResource(node); },
        onCancelTask: cancelNodeTask,
        onOpenTaskDetails: openCanvasNodeTaskDetails,
        onOpenVersions: openCanvasNodeVersions,
        onViewImage: viewCanvasNodeImage,
        onOpenTextEditor: openTextNodeEditor,
        onOpenDirector: editCanvasDirector,
        onOpenDrawing: openDrawingNode,
        onStartBatchConnection: startBatchConnection,
    };

    const { renderCanvasNodePanel, renderCanvasNodeContent } = useCanvasNodeRenderers({
        ...mediaActions,
        ...storyboardActions,
        ...batchActions,
        ...editorActions,
        ...directorActions,
        ...viewportActions,
        ...shortDramaActions,
        ...interactionSetters,
        ...dialogState,
        ...canvasRefs,
        setToolbarNodeId,
        configInputsById,
        confirmStopGeneration,
        directorScenes: currentProject?.directorScenes,
        handleConnectStart,
        handleGenerateNode,
        mentionReferencesByNodeId,
        runningNodeId,
        skillMentionReferences,
        workspaceMode,
    });
    const emptyCanvasState = nodes.length ? null : (
        <CanvasProjectEmptyState
            shortDramaEnabled={shortDramaEnabled}
            currentProject={currentProject}
            linkedProject={linkedProjectQuery.data}
            onUpload={() => handleUploadRequest()}
            onAddText={() => createNode(CanvasNodeType.Text)}
            onAddScript={() => createNode(CanvasNodeType.Script)}
            onAddChapter={handleProjectChapterInsert}
            onOpenAssets={() => openProjectAssets()}
                            onCreatePipeline={shortDramaActions.createShortDramaPipeline}
            onOpenAgent={() => {
                setCinematicAgentEntry(true);
                setAgentMode("online");
                openAgent("online");
            }}
        />
    );
    if (!projectLoaded) return <CanvasRefreshShell />;

    return (
        <>
            <a
                href="#canvas-main"
                className="sr-only focus:not-sr-only focus:absolute focus:left-4 focus:top-4 focus:z-[var(--z-toast)] focus:rounded-md focus:border focus:bg-background focus:px-4 focus:py-2 focus:text-sm focus:font-medium focus:shadow-lg"
            >
                {canvasT("videoCanvas.toast.skipToMain", "跳转到画布主内容")}
            </a>
            <main id="canvas-main" tabIndex={-1} className="flex h-full min-h-0 overflow-hidden outline-none" style={{ background: theme.canvas.background, color: theme.node.text }}>
                {!focusMode && shortDramaEnabled && currentProject?.projectId ? (
                    <CanvasProjectSidebar projectId={currentProject.projectId} detail={linkedProjectQuery.data} onAddChapter={handleProjectChapterInsert} onLocateStyle={locateProjectStyleNode} onOpenAssets={() => openProjectAssets()} />
                ) : null}
                <section className="relative min-w-0 flex-1 flex flex-col min-h-0 overflow-hidden">
                    <CanvasProjectTopChrome
                        {...shortDramaActions}
                        {...viewportActions}
                        {...titleEditingState}
                        {...directorActions}
                        historyActions={historyActions}
                        assistant={assistant}
                        {...dialogState}
                        focusMode={focusMode}
                        currentProject={currentProject}
                        workspaceMode={workspaceMode}
                        setWorkspaceMode={setWorkspaceMode}
                        createAndOpenProject={createAndOpenProject}
                        deleteCurrentProject={deleteCurrentProject}
                        handleUploadRequest={handleUploadRequest}
                        codexCompactAgent={codexCompactAgent}
                        localAgentConnected={localAgentConnected}
                        localAgentEnabled={localAgentEnabled}
                        localAgentActivity={localAgentActivity}
                        mediaPerformanceMode={mediaPerformanceMode}
                        setMediaPerformanceMode={setMediaPerformanceMode}
                        shortDramaEnabled={shortDramaEnabled}
                        canvasContext={canvasContext}
                        linkedProjectQuery={linkedProjectQuery}
                        enterFocusMode={enterFocusMode}
                        shortDramaGuide={shortDramaGuide}
                        nodes={nodes}
                        nodeById={nodeById}
                        toggleFrameCollapsed={toggleFrameCollapsed}
                        toggleBatchExpanded={toggleBatchExpanded}
                        selectedNodeIdsRef={selectedNodeIdsRef}
                        setSelectedNodeIds={setSelectedNodeIds}
                        setSelectedConnectionId={setSelectedConnectionId}
                        activeStylePresetId={activeStylePresetId}
                        selectCanvasStyle={selectCanvasStyle}
                        projectShare={projectShare}
                    />
                    <div className="relative flex min-h-0 min-w-0 flex-1">
                        <CanvasProjectStage
                        {...directorActions}
                        {...canvasInteraction}
                        {...worldLayerHandlers}
                        onReplaceMedia={handleReplaceMedia}
                        renderModel={renderModel}
                        collapsingBatchIds={collapsingBatchIds}
                        openingBatchIds={openingBatchIds}
                        historyActions={historyActions}
                        assistant={assistant}
                        {...dialogState}
                            projectId={projectId}
                            selectedConnectionId={selectedConnectionId}
                            connections={connections}
                            selectedNodeIds={selectedNodeIds}
                            showImageInfo={showImageInfo}
                            emotionNodeId={emotionNodeId}
                            batchSourceNodeIds={batchSourceNodeIds}
                            batchConnectionPreview={batchConnectionPreview}
                            selectionBoundsElementRef={selectionBoundsElementRef}
                            renderCanvasNodeContent={renderCanvasNodeContent}
                            viewport={viewport}
                            theme={theme}
                            containerRef={containerRef}
                            backgroundMode={backgroundMode}
                            canvasTool={canvasTool}
                            activeTasks={activeTasks}
                            focusMode={focusMode}
                            focusDockRevealed={focusDockRevealed}
                            setFocusDockRevealed={setFocusDockRevealed}
                            fileDropActive={fileDropActive}
                            emptyCanvasState={emptyCanvasState}
                            workspaceMode={workspaceMode}
                            setCanvasTool={setCanvasTool}
                            shortDramaEnabled={shortDramaEnabled}
                            currentProject={currentProject}
                            setBackgroundMode={setBackgroundMode}
                            setShowImageInfo={setShowImageInfo}
                            createNode={createNode}
                            createFolder={createFolder}
                            handleUploadRequest={handleUploadRequest}
                            deleteNodes={deleteNodes}
                            openAssetsAtPosition={openAssetsAtPosition}
                            openProjectAssets={openProjectAssets}
                            updateNodeMetadata={(nodeId, patch) => setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, ...patch } } : node)))}
                        />
                        <CanvasProjectAssistantColumn
                        {...dialogState}
                        assistant={assistant}
                        agentOps={agentOps}
                            assistantWidth={assistantWidth}
                            setAssistantWidth={setAssistantWidth}
                            focusMode={focusMode}
                            nodes={nodes}
                            selectedNodeIds={selectedNodeIds}
                            projectId={projectId}
                            chatSessions={chatSessions}
                            activeChatId={activeChatId}
                            setSelectedNodeIds={setSelectedNodeIds}
                            handleAssistantSessionsChange={handleAssistantSessionsChange}
                            pasteAssistantImage={pasteAssistantImage}
                            codexAutoConnect={codexAutoConnect}
                        />
                    </div>
                    {/* 选区框、连接草稿与节点弹层（HideWhileSelectionBox/HideWhileNodeDragging 隔离）统一在此编排 */}
                    <CanvasProjectOverlays
                        {...mediaActions}
                        renderModel={renderModel}
                        dialogNode={dialogNode}
                        viewport={viewport}
                        containerRef={containerRef}
                        setAngleNodeId={setAngleNodeId}
                        setEmotionNodeId={setEmotionNodeId}
                        renderCanvasNodePanel={renderCanvasNodePanel}
                        pendingConnectionCreate={pendingConnectionCreate}
                        size={size}
                        canCreateDrawingFromConnection={canCreateDrawingFromConnection}
                        createConnectedNode={createConnectedNode}
                        cancelPendingConnectionCreate={cancelPendingConnectionCreate}
                        selectionBoundsElementRef={selectionBoundsElementRef}
                        alignSelectedNodes={alignSelectedNodes}
                        arrangeSelectedNodes={arrangeSelectedNodes}
                        createStoryboardGroup={createStoryboardGroup}
                        createReferenceGroup={createReferenceGroup}
                        beginBatchConnectionMode={beginBatchConnectionMode}
                        selectedNodeIds={selectedNodeIds}
                    />
                    <CanvasProjectCanvasChrome
                        {...mediaActions}
                        {...directorActions}
                        {...viewportActions}
                        {...editorActions}
                        {...canvasInteraction}
                        renderModel={renderModel}
                        {...mediaSetters}
                        historyActions={historyActions}
                        agentOps={agentOps}
                        {...dialogState}
                        theme={theme}
                        uploadStatus={uploadStatus}
                        mergeVideoProgress={mergeVideoProgress}
                        nodeImageSettingsOpen={nodeImageSettingsOpen}
                        emotionNodeId={emotionNodeId}
                        workspaceMode={workspaceMode}
                        viewport={viewport}
                        containerRef={containerRef}
                        keepNodeToolbar={keepNodeToolbar}
                        hideNodeToolbar={hideNodeToolbar}
                        openTextNodeEditor={openTextNodeEditor}
                        openDrawingNode={openDrawingNode}
                        generateImageFromTextNode={generateImageFromTextNode}
                        handleUploadRequest={handleUploadRequest}
                        handleRetryNode={handleRetryNode}
                        deleteNodes={deleteNodes}
                        isMiniMapOpen={isMiniMapOpen}
                        focusMode={focusMode}
                        nodes={nodes}
                        size={size}
                        setIsMiniMapOpen={setIsMiniMapOpen}
                        currentProject={currentProject}
                        selectedNodeIds={selectedNodeIds}
                        createMediaAssetNode={createMediaAssetNode}
                        contextMenu={contextMenu}
                        shortDramaEnabled={shortDramaEnabled}
                        hasCopiedNodes={hasCopiedNodes}
                        setContextMenu={setContextMenu}
                        createNode={createNode}
                        createFolder={createFolder}
                        setStylePickerOpen={setStylePickerOpen}
                        openAssetsAtPosition={openAssetsAtPosition}
                        openProjectAssets={openProjectAssets}
                        pasteAtPosition={pasteAtPosition}
                        copyNodesToClipboard={copyNodesToClipboard}
                        duplicateNode={duplicateNode}
                        setTvCoverNode={setTvCoverNode}
                        deleteConnection={deleteConnection}
                        copyNodeContentToClipboard={copyNodeContentToClipboard}
                        copyNodeMediaUrlToClipboard={copyNodeMediaUrlToClipboard}
                        handleConfigNodeChange={handleConfigNodeChange}
                        toggleFrameCollapsed={toggleFrameCollapsed}
                    />
                    <CanvasProjectDialogs
                        {...viewportActions}
                        {...directorActions}
                        {...storyboardActions}
                        {...editorActions}
                        {...mediaDialogs}
                        renderModel={renderModel}
                        {...mediaSetters}
                        assistant={assistant}
                        agentOps={agentOps}
                        {...dialogState}
                        imageInputRef={imageInputRef}
                        handleImageInputChange={handleImageInputChange}
                        nodeById={nodeById}
                        nodes={nodes}
                        currentProject={currentProject}
                        updateProject={updateProject}
                        characterReferenceNode={characterReferenceNode}
                        textEditorNode={textEditorNode}
                        setNodes={setNodes}
                        drawingNode={drawingNode}
                        theme={theme}
                        projectId={projectId}
                        message={message}
                        setPrimaryVersion={setPrimaryVersion}
                        taskDetail={taskDetail}
                        taskDetailLogs={taskDetailLogs}
                        taskDetailLoading={taskDetailLoading}
                        setTaskDetail={setTaskDetail}
                        clearCanvas={clearCanvas}
                        assetPickerOpen={assetPickerOpen}
                        handleAssetInsert={handleAssetInsert}
                        closeAssetPicker={closeAssetPicker}
                        linkedProjectQuery={linkedProjectQuery}
                        handleProjectAssetsInsert={handleProjectAssetsInsert}
                        codexCompactAgent={codexCompactAgent}
                        codexAutoConnect={codexAutoConnect}
                        deleteNodes={deleteNodes}
                        directorOnboardingScope={user?.id?.trim() || ""}
                    />
                </section>
            </main>
        </>
    );
}
