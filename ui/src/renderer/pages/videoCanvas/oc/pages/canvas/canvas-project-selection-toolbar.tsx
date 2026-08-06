import type { RefObject } from "react";

import { CanvasSelectionToolbar } from "@oc/components/canvas/canvas-workspace-overlays";
import { FloatingDock } from "@oc/components/ui/aceternity/floating-dock";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { canvasDockStyle } from "@oc/lib/canvas/canvas-aceternity-style";
import { defaultToolbarPrefs, readToolbarPrefs, resolveToolbarEntries, type ToolContext, type ToolbarHandlers } from "@oc/lib/canvas/tool-registry";
import type { CanvasAlignmentMode } from "@oc/lib/canvas/canvas-layout";
import { useThemeStore } from "@oc/stores/use-theme-store";

type CanvasProjectSelectionToolbarProps = {
    anchorRef: RefObject<HTMLDivElement | null>;
    containerRef: RefObject<HTMLDivElement | null>;
    count: number;
    selectedVideoCount: number;
    mergingVideos: boolean;
    onAlign: (mode: CanvasAlignmentMode) => void;
    onArrange: (mode: "row" | "column" | "grid" | "flow") => void;
    onCreateStoryboard: () => void;
    onCreateReferenceGroup: () => void;
    onMergeVideos: () => void;
};

export function CanvasProjectSelectionToolbar({ anchorRef, containerRef, count, selectedVideoCount, mergingVideos, onAlign, onArrange, onCreateStoryboard, onCreateReferenceGroup, onMergeVideos }: CanvasProjectSelectionToolbarProps) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];

    const handlers = {
        onAlign, onArrange, onCreateStoryboard, onCreateReferenceGroup, onMergeVideos,
    } as Partial<ToolbarHandlers> as ToolbarHandlers;

    const ctx: ToolContext = {
        selectedCount: count,
        selectedNodeTypes: new Set(),
        selectedVideoCount,
        canvasTool: "move",
        workspaceMode: "professional",
        isProjectLinked: false,
        canUndo: false,
        canRedo: false,
        extractingVideoFrame: false,
        mergingVideos,
        addPanelOpen: false,
        appearancePanelOpen: false,
        settingsPanelOpen: false,
        handlers,
    };

    const prefs = readToolbarPrefs("selection") ?? defaultToolbarPrefs("selection");
    const items = resolveToolbarEntries("selection", ctx, prefs);

    return (
        <CanvasSelectionToolbar anchorRef={anchorRef} containerRef={containerRef} count={count}>
            <FloatingDock items={items} size="compact" className="canvas-floating-dock" style={canvasDockStyle(theme)} ariaLabel="多选节点布局工具" />
        </CanvasSelectionToolbar>
    );
}
