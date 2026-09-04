import { create } from "zustand";
import type { SetStateAction } from "react";

import type { ConnectionHandle, Position, SelectionBox } from "@oc/types/canvas";

export type CanvasDragPreview = {
    x: number;
    y: number;
    nodeIds: string[];
};

export type CanvasAlignmentGuides = {
    vertical?: number;
    horizontal?: number;
};

function resolveAction<T>(action: SetStateAction<T>, current: T): T {
    return typeof action === "function" ? (action as (value: T) => T)(current) : action;
}

function samePosition(left: Position, right: Position) {
    return left.x === right.x && left.y === right.y;
}

function sameConnectingParams(left: ConnectionHandle | null, right: ConnectionHandle | null) {
    if (left === right) return true;
    if (!left || !right) return false;
    return left.nodeId === right.nodeId && left.handleType === right.handleType && left.handleId === right.handleId && left.anchorRatio === right.anchorRatio;
}

function sameDragPreview(left: CanvasDragPreview | null, right: CanvasDragPreview | null) {
    if (left === right) return true;
    if (!left || !right) return false;
    return left.x === right.x && left.y === right.y && left.nodeIds.length === right.nodeIds.length && left.nodeIds.every((id, index) => id === right.nodeIds[index]);
}

function sameSelectionBox(left: SelectionBox | null, right: SelectionBox | null) {
    if (left === right) return true;
    if (!left || !right) return false;
    return (
        left.startWorldX === right.startWorldX
        && left.startWorldY === right.startWorldY
        && left.currentWorldX === right.currentWorldX
        && left.currentWorldY === right.currentWorldY
        && left.additive === right.additive
        && left.subtractive === right.subtractive
        && left.initialSelectedNodeIds.length === right.initialSelectedNodeIds.length
        && left.initialSelectedNodeIds.every((id, index) => id === right.initialSelectedNodeIds[index])
    );
}

type CanvasInteractionStore = {
    hoveredNodeId: string | null;
    toolbarNodeId: string | null;
    dragPreview: CanvasDragPreview | null;
    isNodeDragging: boolean;
    alignmentGuides: CanvasAlignmentGuides;
    frameDropTargetId: string | null;
    selectionBox: SelectionBox | null;
    mouseWorld: Position;
    connectingParams: ConnectionHandle | null;
    connectionTargetNodeId: string | null;
    connectionTargetAnchorRatio: number | undefined;
    setHoveredNodeId: (id: SetStateAction<string | null>) => void;
    setToolbarNodeId: (id: SetStateAction<string | null>) => void;
    setDragPreview: (preview: SetStateAction<CanvasDragPreview | null>) => void;
    setIsNodeDragging: (value: boolean) => void;
    setAlignmentGuides: (guides: SetStateAction<CanvasAlignmentGuides>) => void;
    setFrameDropTargetId: (id: string | null) => void;
    setSelectionBox: (box: SetStateAction<SelectionBox | null>) => void;
    setMouseWorld: (position: SetStateAction<Position>) => void;
    setConnectingParams: (params: SetStateAction<ConnectionHandle | null>) => void;
    setConnectionTargetNodeId: (id: SetStateAction<string | null>) => void;
    setConnectionTargetAnchorRatio: (ratio: SetStateAction<number | undefined>) => void;
    resetInteraction: () => void;
};

const initialState = {
    hoveredNodeId: null as string | null,
    toolbarNodeId: null as string | null,
    dragPreview: null as CanvasDragPreview | null,
    isNodeDragging: false,
    alignmentGuides: {} as CanvasAlignmentGuides,
    frameDropTargetId: null as string | null,
    selectionBox: null as SelectionBox | null,
    mouseWorld: { x: 0, y: 0 } as Position,
    connectingParams: null as ConnectionHandle | null,
    connectionTargetNodeId: null as string | null,
    connectionTargetAnchorRatio: undefined as number | undefined,
};

export const useCanvasInteractionStore = create<CanvasInteractionStore>((set) => ({
    ...initialState,
    setHoveredNodeId: (id) => set((state) => {
        const hoveredNodeId = resolveAction(id, state.hoveredNodeId);
        return hoveredNodeId === state.hoveredNodeId ? state : { hoveredNodeId };
    }),
    setToolbarNodeId: (id) => set((state) => {
        const toolbarNodeId = resolveAction(id, state.toolbarNodeId);
        return toolbarNodeId === state.toolbarNodeId ? state : { toolbarNodeId };
    }),
    setDragPreview: (preview) => set((state) => {
        const dragPreview = resolveAction(preview, state.dragPreview);
        return sameDragPreview(dragPreview, state.dragPreview) ? state : { dragPreview };
    }),
    setIsNodeDragging: (isNodeDragging) => set((state) => (state.isNodeDragging === isNodeDragging ? state : { isNodeDragging })),
    setAlignmentGuides: (guides) => set((state) => {
        const next = resolveAction(guides, state.alignmentGuides);
        return next.vertical === state.alignmentGuides.vertical && next.horizontal === state.alignmentGuides.horizontal
            ? state
            : { alignmentGuides: next };
    }),
    setFrameDropTargetId: (frameDropTargetId) => set((state) => (state.frameDropTargetId === frameDropTargetId ? state : { frameDropTargetId })),
    setSelectionBox: (box) => set((state) => {
        const selectionBox = resolveAction(box, state.selectionBox);
        return sameSelectionBox(selectionBox, state.selectionBox) ? state : { selectionBox };
    }),
    setMouseWorld: (position) => set((state) => {
        const mouseWorld = resolveAction(position, state.mouseWorld);
        return samePosition(mouseWorld, state.mouseWorld) ? state : { mouseWorld };
    }),
    setConnectingParams: (params) => set((state) => {
        const connectingParams = resolveAction(params, state.connectingParams);
        return sameConnectingParams(connectingParams, state.connectingParams) ? state : { connectingParams };
    }),
    setConnectionTargetNodeId: (id) => set((state) => {
        const connectionTargetNodeId = resolveAction(id, state.connectionTargetNodeId);
        return connectionTargetNodeId === state.connectionTargetNodeId ? state : { connectionTargetNodeId };
    }),
    setConnectionTargetAnchorRatio: (ratio) => set((state) => {
        const connectionTargetAnchorRatio = resolveAction(ratio, state.connectionTargetAnchorRatio);
        return connectionTargetAnchorRatio === state.connectionTargetAnchorRatio ? state : { connectionTargetAnchorRatio };
    }),
    resetInteraction: () => set(initialState),
}));
