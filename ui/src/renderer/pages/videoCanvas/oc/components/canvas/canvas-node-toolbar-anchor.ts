/** Screen-space gap between the toolbar bottom and the node card top. */
export const CANVAS_NODE_TOOLBAR_GAP_PX = 6;

type RectLike = {
    left: number;
    top: number;
    width: number;
    right?: number;
};

/**
 * Places the node action bar in viewport coordinates so `-translate-y-full`
 * puts its bottom edge just above the node card, not above the external title.
 */
export function computeCanvasNodeToolbarAnchor(input: {
    nodeRect: RectLike;
    containerRect: RectLike & { right?: number };
    toolbarWidth: number;
    gap?: number;
}): { left: number; top: number } {
    const gap = input.gap ?? CANVAS_NODE_TOOLBAR_GAP_PX;
    const containerRight = input.containerRect.right ?? input.containerRect.left + input.containerRect.width;
    const preferredLeft = input.nodeRect.left + input.nodeRect.width / 2;
    const halfToolbar = input.toolbarWidth / 2;
    const canClamp = input.toolbarWidth > 0 && input.toolbarWidth <= input.containerRect.width - 20;
    const left = Math.round(
        canClamp
            ? Math.min(Math.max(preferredLeft, input.containerRect.left + halfToolbar + 10), containerRight - halfToolbar - 10)
            : preferredLeft,
    );
    const top = Math.round(input.nodeRect.top - gap);
    return { left, top };
}
