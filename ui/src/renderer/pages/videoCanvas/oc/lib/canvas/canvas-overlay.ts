import type { CSSProperties } from "react";

import type { CanvasTheme } from "@oc/lib/canvas-theme";

export type OverlayPlacement = "topLeft" | "top" | "topRight" | "bottomLeft" | "bottom" | "bottomRight";

export function canvasOverlayStyle(theme: CanvasTheme, extra?: CSSProperties): CSSProperties {
    return {
        background: theme.spatial.elevated,
        borderColor: theme.toolbar.border,
        color: theme.node.text,
        boxShadow: `0 10px 32px ${theme.spatial.shadow}`,
        ...extra,
    };
}

export function anchoredOverlayStyle(
    buttonRect: { left: number; right: number; top: number; bottom: number; width: number },
    viewport: { width: number; height: number },
    options: {
        width: number;
        placement?: OverlayPlacement;
        gap?: number;
        margin?: number;
        estimatedHeight?: number;
        maxHeightFloor?: number;
    },
): CSSProperties {
    const gap = options.gap ?? 8;
    const margin = options.margin ?? 12;
    const width = Math.min(options.width, viewport.width - margin * 2);
    const placement = options.placement ?? "topLeft";
    const alignRight = placement.endsWith("Right");
    const alignCenter = placement === "top" || placement === "bottom";
    const unclampedLeft = alignCenter ? buttonRect.left + buttonRect.width / 2 - width / 2 : alignRight ? buttonRect.right - width : buttonRect.left;
    const left = Math.max(margin, Math.min(viewport.width - width - margin, unclampedLeft));
    const estimatedHeight = options.estimatedHeight ?? 320;
    const topSpace = buttonRect.top - gap - margin;
    const bottomSpace = viewport.height - buttonRect.bottom - gap - margin;
    const preferAbove = placement.startsWith("top");
    const placeAbove = preferAbove ? topSpace >= estimatedHeight || topSpace >= bottomSpace : bottomSpace < estimatedHeight && topSpace > bottomSpace;
    const floor = options.maxHeightFloor ?? 260;
    return {
        position: "fixed",
        zIndex: "var(--z-popover)",
        width,
        left,
        ...(placeAbove
            ? { bottom: viewport.height - buttonRect.top + gap, maxHeight: Math.max(floor, topSpace) }
            : { top: buttonRect.bottom + gap, maxHeight: Math.max(floor, bottomSpace) }),
    };
}
