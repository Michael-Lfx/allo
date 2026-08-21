import { create } from "zustand";
import type { SetStateAction } from "react";

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

type CanvasInteractionStore = {
    hoveredNodeId: string | null;
    toolbarNodeId: string | null;
    dragPreview: CanvasDragPreview | null;
    isNodeDragging: boolean;
    alignmentGuides: CanvasAlignmentGuides;
    frameDropTargetId: string | null;
    setHoveredNodeId: (id: SetStateAction<string | null>) => void;
    setToolbarNodeId: (id: SetStateAction<string | null>) => void;
    setDragPreview: (preview: SetStateAction<CanvasDragPreview | null>) => void;
    setIsNodeDragging: (value: boolean) => void;
    setAlignmentGuides: (guides: SetStateAction<CanvasAlignmentGuides>) => void;
    setFrameDropTargetId: (id: string | null) => void;
    resetInteraction: () => void;
};

const initialState = {
    hoveredNodeId: null as string | null,
    toolbarNodeId: null as string | null,
    dragPreview: null as CanvasDragPreview | null,
    isNodeDragging: false,
    alignmentGuides: {} as CanvasAlignmentGuides,
    frameDropTargetId: null as string | null,
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
    setDragPreview: (preview) => set((state) => ({ dragPreview: resolveAction(preview, state.dragPreview) })),
    setIsNodeDragging: (isNodeDragging) => set((state) => (state.isNodeDragging === isNodeDragging ? state : { isNodeDragging })),
    setAlignmentGuides: (guides) => set((state) => {
        const next = resolveAction(guides, state.alignmentGuides);
        return next.vertical === state.alignmentGuides.vertical && next.horizontal === state.alignmentGuides.horizontal
            ? state
            : { alignmentGuides: next };
    }),
    setFrameDropTargetId: (frameDropTargetId) => set((state) => (state.frameDropTargetId === frameDropTargetId ? state : { frameDropTargetId })),
    resetInteraction: () => set(initialState),
}));
