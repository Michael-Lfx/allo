import type { CSSProperties } from "react";

import type { CanvasTheme } from "@oc/lib/canvas-theme";

export function canvasDockStyle(theme: CanvasTheme, color: string = theme.toolbar.item): CSSProperties {
    return {
        background: theme.spatial.elevated,
        borderColor: theme.toolbar.border,
        color,
        boxShadow: `0 8px 28px ${theme.spatial.shadow}`,
        "--dock-command-bg": theme.spatial.surface,
        "--dock-command-hover": theme.toolbar.itemHover,
        "--dock-command-active": theme.toolbar.activeBg,
        "--dock-command-active-text": theme.toolbar.activeText,
        "--dock-command-danger": theme.accent.danger,
        "--dock-tooltip-bg": theme.spatial.elevated,
        "--dock-tooltip-border": theme.toolbar.border,
    } as CSSProperties;
}
