import { useCallback, type Dispatch, type SetStateAction } from "react";
import { App } from "antd";

import type { CanvasStylePreset } from "@oc/components/canvas/canvas-style-picker-modal";
import { createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import { CanvasNodeType, type CanvasNodeData, type CanvasNodeMetadata, type Position } from "@oc/types/canvas";

type UseCanvasStyleWorkflowOptions = {
    nodesRef: { current: CanvasNodeData[] };
    selectedNodeIdsRef: { current: Set<string> };
    getCanvasCenter: () => Position;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setStylePickerOpen: Dispatch<SetStateAction<boolean>>;
};

export function useCanvasStyleWorkflow({
    nodesRef,
    selectedNodeIdsRef,
    getCanvasCenter,
    setNodes,
    setSelectedNodeIds,
    setSelectedConnectionId,
    setDialogNodeId,
    setStylePickerOpen,
}: UseCanvasStyleWorkflowOptions) {
    const { message } = App.useApp();

    const selectCanvasStyle = useCallback((preset: CanvasStylePreset) => {
        const current = nodesRef.current.find((node) => node.type === CanvasNodeType.Text && node.metadata?.workflowKind === "styleboard");
        const metadata: CanvasNodeMetadata = {
            content: preset.prompt,
            prompt: preset.prompt,
            status: "success",
            workflowKind: "styleboard",
            workflowTitle: "项目画风",
            workflowDescription: preset.description,
            stylePresetId: preset.id,
            fontSize: 14,
        };
        let styleNode: CanvasNodeData;
        if (current) {
            styleNode = {
                ...current,
                title: `项目画风 · ${preset.title}`,
                width: Math.max(current.width, 420),
                height: Math.max(current.height, 280),
                metadata: { ...current.metadata, ...metadata },
            };
            nodesRef.current = nodesRef.current.map((node) => node.id === current.id ? styleNode : node);
        } else {
            styleNode = createCanvasNode(CanvasNodeType.Text, getCanvasCenter(), metadata);
            styleNode.title = `项目画风 · ${preset.title}`;
            styleNode.width = 420;
            styleNode.height = 280;
            nodesRef.current = [...nodesRef.current, styleNode];
        }
        setNodes(nodesRef.current);
        const selection = new Set([styleNode.id]);
        selectedNodeIdsRef.current = selection;
        setSelectedNodeIds(selection);
        setSelectedConnectionId(null);
        setDialogNodeId(null);
        setStylePickerOpen(false);
        message.success(`已应用“${preset.title}”画风`);
    }, [getCanvasCenter, message, nodesRef, selectedNodeIdsRef, setDialogNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds, setStylePickerOpen]);

    return { selectCanvasStyle };
}
