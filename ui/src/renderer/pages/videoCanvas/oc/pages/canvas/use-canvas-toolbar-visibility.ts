import { useCallback, useRef } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";

type CanvasToolbarVisibilityInput = {
    nodeDraggingRef: RefObject<boolean>;
    nodeImageSettingsOpen: boolean;
    setHoveredNodeId: Dispatch<SetStateAction<string | null>>;
    setToolbarNodeId: Dispatch<SetStateAction<string | null>>;
};

export function useCanvasToolbarVisibility(input: CanvasToolbarVisibilityInput) {
    const { nodeDraggingRef, nodeImageSettingsOpen, setHoveredNodeId, setToolbarNodeId } = input;
    const toolbarHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const keepNodeToolbar = useCallback(
        (nodeId: string) => {
            if (nodeDraggingRef.current || nodeImageSettingsOpen) return;
            if (toolbarHideTimerRef.current) {
                clearTimeout(toolbarHideTimerRef.current);
                toolbarHideTimerRef.current = null;
            }
            setToolbarNodeId(nodeId);
        },
        [nodeImageSettingsOpen],
    );

    const hideNodeToolbar = useCallback(() => {
        if (toolbarHideTimerRef.current) clearTimeout(toolbarHideTimerRef.current);
        toolbarHideTimerRef.current = setTimeout(() => {
            setToolbarNodeId(null);
            toolbarHideTimerRef.current = null;
        }, 120);
    }, []);

    const handleCanvasNodeHoverStart = useCallback(
        (nodeId: string) => {
            if (nodeDraggingRef.current) return;
            setHoveredNodeId(nodeId);
            keepNodeToolbar(nodeId);
        },
        [keepNodeToolbar],
    );

    const handleCanvasNodeHoverEnd = useCallback(
        (nodeId: string) => {
            setHoveredNodeId((current) => (current === nodeId ? null : current));
            hideNodeToolbar();
        },
        [hideNodeToolbar],
    );
    return { keepNodeToolbar, hideNodeToolbar, handleCanvasNodeHoverStart, handleCanvasNodeHoverEnd };
}
