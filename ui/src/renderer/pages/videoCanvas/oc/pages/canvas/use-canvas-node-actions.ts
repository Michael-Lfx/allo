import { useCallback } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { App } from "antd";
import { CanvasNodeType, type CanvasNodeData, type ContextMenuState } from "@oc/types/canvas";
import type { useCanvasGeneration } from "./use-canvas-generation";
import type { useCanvasUpload } from "./use-canvas-upload";
import type { useCanvasViewportController } from "./use-canvas-viewport-controller";

type CanvasNodeActionsInput = {
    setContextMenu: Dispatch<SetStateAction<ContextMenuState | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setHoveredNodeId: Dispatch<SetStateAction<string | null>>;
    setToolbarNodeId: Dispatch<SetStateAction<string | null>>;
    setDrawingNodeId: Dispatch<SetStateAction<string | null>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setCharacterReferenceNodeId: Dispatch<SetStateAction<string | null>>;
    setTextEditorNodeId: Dispatch<SetStateAction<string | null>>;
    setVersionCompareRootId: Dispatch<SetStateAction<string | null>>;
    setPreviewNodeId: Dispatch<SetStateAction<string | null>>;
    openNodeTaskDetails: ReturnType<typeof useCanvasGeneration>["openNodeTaskDetails"];
    handleUploadRequest: ReturnType<typeof useCanvasUpload>["handleUploadRequest"];
    nodesRef: RefObject<CanvasNodeData[]>;
    focusCanvasNode: ReturnType<typeof useCanvasViewportController>["focusCanvasNode"];
    message: ReturnType<typeof App.useApp>["message"];
};

export function useCanvasNodeActions(input: CanvasNodeActionsInput) {
    const {
        setContextMenu,
        setDialogNodeId,
        setHoveredNodeId,
        setToolbarNodeId,
        setDrawingNodeId,
        setSelectedNodeIds,
        setSelectedConnectionId,
        setCharacterReferenceNodeId,
        setTextEditorNodeId,
        setVersionCompareRootId,
        setPreviewNodeId,
        openNodeTaskDetails,
        handleUploadRequest,
        nodesRef,
        focusCanvasNode,
        message,
    } = input;

    const handleCanvasSelectionStart = useCallback(() => {
        setContextMenu(null);
        setDialogNodeId(null);
    }, []);
    const handleNodeInteractionStart = useCallback((selectionModifier: boolean) => {
        setContextMenu(null);
        setHoveredNodeId(null);
        setToolbarNodeId(null);
        if (selectionModifier) setDialogNodeId(null);
    }, []);
    const handleSelectedNodeClick = useCallback((node: CanvasNodeData) => {
        if (node.type === CanvasNodeType.Drawing) {
            setDialogNodeId(null);
            setDrawingNodeId(node.id);
        } else if (node.type === CanvasNodeType.Script) {
            setDialogNodeId(null);
        } else if (node.type === CanvasNodeType.ArtCritique) {
            setDialogNodeId(null);
        } else if (node.type === CanvasNodeType.Text || node.type === CanvasNodeType.Frame) {
            setDialogNodeId((current) => (current === node.id ? current : null));
        } else {
            setDialogNodeId(node.id);
        }
    }, []);
    const handleCanvasDeselect = useCallback(() => {
        setContextMenu(null);
        setHoveredNodeId(null);
        setToolbarNodeId(null);
        setDialogNodeId(null);
    }, []);
    const openTextNodeEditor = useCallback((node: CanvasNodeData) => {
        if (node.type !== CanvasNodeType.Text) return;
        setSelectedNodeIds(new Set([node.id]));
        setSelectedConnectionId(null);
        setContextMenu(null);
        setDialogNodeId(null);
        setToolbarNodeId(null);
        if (node.metadata?.workflowKind === "character" && node.metadata.characterAssetId) {
            setCharacterReferenceNodeId(node.id);
            return;
        }
        setTextEditorNodeId(node.id);
    }, []);
    const openDrawingNode = useCallback((node: CanvasNodeData) => {
        if (node.type !== CanvasNodeType.Drawing) return;
        setSelectedNodeIds(new Set([node.id]));
        setSelectedConnectionId(null);
        setContextMenu(null);
        setDialogNodeId(null);
        setToolbarNodeId(null);
        setDrawingNodeId(node.id);
    }, []);
    const openCanvasNodeTaskDetails = useCallback(
        (node: CanvasNodeData) => {
            void openNodeTaskDetails(node);
        },
        [openNodeTaskDetails],
    );
    const openCanvasNodeVersions = useCallback((node: CanvasNodeData) => setVersionCompareRootId(node.metadata?.versionOfNodeId || node.id), []);
    const viewCanvasNodeImage = useCallback((node: CanvasNodeData) => setPreviewNodeId(node.id), []);
    const handleReplaceMedia = useCallback((node: CanvasNodeData) => {
        handleUploadRequest(node.id);
    }, [handleUploadRequest]);
    const locateProjectStyleNode = useCallback(() => {
        const styleNode = nodesRef.current.find((node) => node.type === CanvasNodeType.Text && node.metadata?.workflowKind === "styleboard");
        if (!styleNode) {
            message.info(canvasT("videoCanvas.toast.styleSyncing", "项目画风节点正在同步，请稍后再试"));
            return;
        }
        focusCanvasNode(styleNode.id);
    }, [focusCanvasNode, message, nodesRef]);

    return {
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
    };
}