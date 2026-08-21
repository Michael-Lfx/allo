import { describe, expect, test } from "bun:test";

import { useCanvasInteractionStore } from "./use-canvas-interaction-store";

describe("canvas interaction store", () => {
    test("stores hover and toolbar ids without sharing the gallery ui store", () => {
        useCanvasInteractionStore.getState().resetInteraction();
        const store = useCanvasInteractionStore.getState();
        store.setHoveredNodeId("node-a");
        store.setToolbarNodeId("node-b");

        expect(useCanvasInteractionStore.getState().hoveredNodeId).toBe("node-a");
        expect(useCanvasInteractionStore.getState().toolbarNodeId).toBe("node-b");

        store.setHoveredNodeId((current) => (current === "node-a" ? null : current));
        expect(useCanvasInteractionStore.getState().hoveredNodeId).toBeNull();
        expect(useCanvasInteractionStore.getState().toolbarNodeId).toBe("node-b");
    });

    test("stores drag preview with nodeIds as an array", () => {
        useCanvasInteractionStore.getState().resetInteraction();
        useCanvasInteractionStore.getState().setDragPreview({ x: 12, y: -4, nodeIds: ["a", "b"] });
        useCanvasInteractionStore.getState().setIsNodeDragging(true);
        useCanvasInteractionStore.getState().setAlignmentGuides({ vertical: 40 });
        useCanvasInteractionStore.getState().setFrameDropTargetId("frame-1");

        const state = useCanvasInteractionStore.getState();
        expect(state.dragPreview).toEqual({ x: 12, y: -4, nodeIds: ["a", "b"] });
        expect(Array.isArray(state.dragPreview?.nodeIds)).toBe(true);
        expect(state.isNodeDragging).toBe(true);
        expect(state.alignmentGuides).toEqual({ vertical: 40 });
        expect(state.frameDropTargetId).toBe("frame-1");

        state.setDragPreview((current) => (current ? { ...current, x: 20, y: current.y } : current));
        expect(useCanvasInteractionStore.getState().dragPreview?.x).toBe(20);
        expect(useCanvasInteractionStore.getState().dragPreview?.nodeIds).toEqual(["a", "b"]);
    });
});
