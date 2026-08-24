import { useCallback } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { CanvasNodeData, ContextMenuState } from "@oc/types/canvas";
import type { useCanvasViewportController } from "./use-canvas-viewport-controller";

type CanvasContextMenuActionsInput = {
    closeConnectionCreateMenu: () => void;
    screenToCanvas: ReturnType<typeof useCanvasViewportController>["screenToCanvas"];
    setContextMenu: Dispatch<SetStateAction<ContextMenuState | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setHoveredNodeId: Dispatch<SetStateAction<string | null>>;
    setToolbarNodeId: Dispatch<SetStateAction<string | null>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
};

export function useCanvasContextMenuActions(input: CanvasContextMenuActionsInput) {
    const { closeConnectionCreateMenu, screenToCanvas, setContextMenu, setDialogNodeId, setHoveredNodeId, setToolbarNodeId, setSelectedConnectionId, setSelectedNodeIds } = input;

    const handleCanvasContextMenu = useCallback(
        (event: ReactMouseEvent) => {
            const target = event.target instanceof Element ? event.target : null;
            if (target?.closest("[data-node-id],[data-connection-id]")) return;

            event.preventDefault();
            event.stopPropagation();
            if (target?.closest("[data-canvas-no-zoom],.ant-modal,.ant-popover,.ant-dropdown")) {
                setContextMenu(null);
                return;
            }

            closeConnectionCreateMenu();
            setContextMenu({ type: "canvas", x: event.clientX, y: event.clientY, position: screenToCanvas(event.clientX, event.clientY) });
        },
        [closeConnectionCreateMenu, screenToCanvas],
    );

    const handleNodeContextMenu = useCallback(
        (event: ReactMouseEvent, id: string) => {
            event.preventDefault();
            event.stopPropagation();
            setSelectedNodeIds(new Set([id]));
            setSelectedConnectionId(null);
            closeConnectionCreateMenu();
            setToolbarNodeId(null);
            setDialogNodeId(null);
            setContextMenu({ type: "node", x: event.clientX, y: event.clientY, nodeId: id });
        },
        [closeConnectionCreateMenu],
    );

    const handleConnectionSelect = useCallback((connectionId: string) => {
        setSelectedConnectionId(connectionId);
        setSelectedNodeIds(new Set());
        setContextMenu(null);
    }, []);

    const handleConnectionContextMenu = useCallback(
        (event: ReactMouseEvent<SVGPathElement>, connectionId: string) => {
            setSelectedConnectionId(connectionId);
            setSelectedNodeIds(new Set());
            closeConnectionCreateMenu();
            setContextMenu({ type: "connection", x: event.clientX, y: event.clientY, connectionId });
        },
        [closeConnectionCreateMenu],
    );
    return { handleCanvasContextMenu, handleNodeContextMenu, handleConnectionSelect, handleConnectionContextMenu };
}
