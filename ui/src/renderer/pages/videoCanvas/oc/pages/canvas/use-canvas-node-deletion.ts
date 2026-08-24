import { useCallback } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
import { removeCanvasDrawing } from "@oc/lib/canvas/canvas-drawing-storage";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { App } from "antd";
import { CanvasNodeType, type CanvasAssistantSession, type CanvasConnection, type CanvasNodeData, type ContextMenuState } from "@oc/types/canvas";
import type { useCanvasProjectLifecycle } from "./use-canvas-project-lifecycle";

type SetNodeId = Dispatch<SetStateAction<string | null>>;
type CanvasNodeDeletionInput = {
    chatSessions: CanvasAssistantSession[];
    cleanupCanvasFiles: (extra?: unknown) => void;
    projectId: string;
    message: ReturnType<typeof App.useApp>["message"];
    setHoveredNodeId: SetNodeId;
    setToolbarNodeId: SetNodeId;
    setDialogNodeId: SetNodeId;
    setTextEditorNodeId: SetNodeId;
    setCharacterReferenceNodeId: SetNodeId;
    setDrawingNodeId: SetNodeId;
    setInfoNodeId: SetNodeId;
    setCropNodeId: SetNodeId;
    setMaskEditNodeId: SetNodeId;
    setAnnotationNodeId: SetNodeId;
    setSplitNodeId: SetNodeId;
    setUpscaleNodeId: SetNodeId;
    setAngleNodeId: SetNodeId;
    setEmotionNodeId: SetNodeId;
    setSuperResolveNodeId: SetNodeId;
    setPreviewNodeId: SetNodeId;
    setRunningNodeId: SetNodeId;
    setScriptEditorNodeId: SetNodeId;
    setDirectorNodeId: SetNodeId;
    setVersionCompareRootId: SetNodeId;
    setScriptScrollTopById: Dispatch<SetStateAction<Record<string, number>>>;
    setContextMenu: Dispatch<SetStateAction<ContextMenuState | null>>;
};

export function useCanvasNodeDeletion(input: CanvasNodeDeletionInput) {
    const {
        chatSessions,
        cleanupCanvasFiles,
        projectId,
        message,
        setHoveredNodeId,
        setToolbarNodeId,
        setDialogNodeId,
        setTextEditorNodeId,
        setCharacterReferenceNodeId,
        setDrawingNodeId,
        setInfoNodeId,
        setCropNodeId,
        setMaskEditNodeId,
        setAnnotationNodeId,
        setSplitNodeId,
        setUpscaleNodeId,
        setAngleNodeId,
        setEmotionNodeId,
        setSuperResolveNodeId,
        setPreviewNodeId,
        setRunningNodeId,
        setScriptEditorNodeId,
        setDirectorNodeId,
        setVersionCompareRootId,
        setScriptScrollTopById,
        setContextMenu,
    } = input;

    const handleNodesDeleted = useCallback(
        (removedIds: Set<string>, nextNodes: CanvasNodeData[], removedNodes: CanvasNodeData[]) => {
            const clearDeletedId = (current: string | null) => (current && removedIds.has(current) ? null : current);
            setHoveredNodeId(clearDeletedId);
            setToolbarNodeId(clearDeletedId);
            setDialogNodeId(clearDeletedId);
            setTextEditorNodeId(clearDeletedId);
            setCharacterReferenceNodeId(clearDeletedId);
            setDrawingNodeId(clearDeletedId);
            setInfoNodeId(clearDeletedId);
            setCropNodeId(clearDeletedId);
            setMaskEditNodeId(clearDeletedId);
            setAnnotationNodeId(clearDeletedId);
            setSplitNodeId(clearDeletedId);
            setUpscaleNodeId(clearDeletedId);
            setAngleNodeId(clearDeletedId);
            setEmotionNodeId(clearDeletedId);
            setSuperResolveNodeId(clearDeletedId);
            setPreviewNodeId(clearDeletedId);
            setRunningNodeId(clearDeletedId);
            setScriptEditorNodeId(clearDeletedId);
            setDirectorNodeId(clearDeletedId);
            setVersionCompareRootId(clearDeletedId);
            setScriptScrollTopById((current) => Object.fromEntries(Object.entries(current).filter(([id]) => !removedIds.has(id))));
            setContextMenu((current) => (current?.type === "node" && removedIds.has(current.nodeId) ? null : current));
            const removedDrawingIds = removedNodes.flatMap((node) => (node.type === CanvasNodeType.Drawing && node.metadata?.drawingId ? [node.metadata.drawingId] : []));
            if (removedDrawingIds.length) {
                void Promise.all(removedDrawingIds.map((drawingId) => removeCanvasDrawing(projectId, drawingId))).catch(() => message.warning(canvasT("videoCanvas.toast.drawingCacheCleanFail", "绘图节点已删除，但本地绘图缓存清理失败")));
            }
            cleanupCanvasFiles({ projectId, nodes: nextNodes, chatSessions });
        },
        [chatSessions, cleanupCanvasFiles, message, projectId, setAngleNodeId, setAnnotationNodeId, setCropNodeId, setEmotionNodeId, setMaskEditNodeId, setSplitNodeId, setUpscaleNodeId, setRunningNodeId],
    );

    return { handleNodesDeleted };
}

type CanvasClearCanvasInput = {
    nodesRef: RefObject<CanvasNodeData[]>;
    projectId: string;
    message: ReturnType<typeof App.useApp>["message"];
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setTextEditorNodeId: SetNodeId;
    setDrawingNodeId: SetNodeId;
    setInfoNodeId: SetNodeId;
    setCropNodeId: SetNodeId;
    setMaskEditNodeId: SetNodeId;
    setAnnotationNodeId: SetNodeId;
    setAngleNodeId: SetNodeId;
    setEmotionNodeId: SetNodeId;
    setPreviewNodeId: SetNodeId;
    setRunningNodeId: SetNodeId;
    setClearConfirmOpen: Dispatch<SetStateAction<boolean>>;
    deselectCanvas: () => void;
    clearCanvasFiles: ReturnType<typeof useCanvasProjectLifecycle>["clearCanvasFiles"];
};

export function useCanvasClearCanvas(input: CanvasClearCanvasInput) {
    const {
        nodesRef,
        projectId,
        message,
        setNodes,
        setConnections,
        setTextEditorNodeId,
        setDrawingNodeId,
        setInfoNodeId,
        setCropNodeId,
        setMaskEditNodeId,
        setAnnotationNodeId,
        setAngleNodeId,
        setEmotionNodeId,
        setPreviewNodeId,
        setRunningNodeId,
        setClearConfirmOpen,
        deselectCanvas,
        clearCanvasFiles,
    } = input;

    const clearCanvas = useCallback(() => {
        const drawingIds = nodesRef.current.flatMap((node) => (node.type === CanvasNodeType.Drawing && node.metadata?.drawingId ? [node.metadata.drawingId] : []));
        if (drawingIds.length) {
            void Promise.all(drawingIds.map((drawingId) => removeCanvasDrawing(projectId, drawingId))).catch(() => message.warning(canvasT("videoCanvas.toast.canvasClearedCacheFail", "画布已清空，但部分本地绘图缓存清理失败")));
        }
        setNodes([]);
        setConnections([]);
        setTextEditorNodeId(null);
        setDrawingNodeId(null);
        setInfoNodeId(null);
        setCropNodeId(null);
        setMaskEditNodeId(null);
        setAnnotationNodeId(null);
        setAngleNodeId(null);
        setEmotionNodeId(null);
        setPreviewNodeId(null);
        setRunningNodeId(null);
        deselectCanvas();
        setClearConfirmOpen(false);
        clearCanvasFiles();
    }, [clearCanvasFiles, deselectCanvas, message, nodesRef, projectId, setEmotionNodeId]);

    return { clearCanvas };
}
