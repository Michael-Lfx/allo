/** Screen-space gap between the toolbar bottom and the node card top. */
export const CANVAS_NODE_TOOLBAR_GAP_PX = 6;

type RectLike = {
    left: number;
    top: number;
    width: number;
    right?: number;
};

/** Viewport rect when `[data-node-id]` is not in the canvas container yet. */
export function canvasNodeFallbackScreenRect(input: {
    node: { position: { x: number; y: number }; width: number };
    viewport: { x: number; y: number; k: number };
    containerRect: { left: number; top: number };
}): { left: number; top: number; width: number } {
    return {
        left: input.containerRect.left + input.viewport.x + input.node.position.x * input.viewport.k,
        top: input.containerRect.top + input.viewport.y + input.node.position.y * input.viewport.k,
        width: input.node.width * input.viewport.k,
    };
}

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

/** Live screen-space anchor from the node DOM, or world-position fallback. */
export function readCanvasNodeToolbarAnchor(input: {
    node: { id: string; position: { x: number; y: number }; width: number };
    viewport: { x: number; y: number; k: number };
    container: HTMLElement;
    toolbarWidth: number;
}): { left: number; top: number } {
    const containerRect = input.container.getBoundingClientRect();
    const escapedId = typeof CSS !== "undefined" && typeof CSS.escape === "function"
        ? CSS.escape(input.node.id)
        : input.node.id.replace(/["\\]/g, "\\$&");
    const element = input.container.querySelector<HTMLElement>(`[data-node-id="${escapedId}"]`);
    const nodeRect = element
        ? element.getBoundingClientRect()
        : canvasNodeFallbackScreenRect({ node: input.node, viewport: input.viewport, containerRect });
    return computeCanvasNodeToolbarAnchor({
        nodeRect,
        containerRect,
        toolbarWidth: input.toolbarWidth,
    });
}
