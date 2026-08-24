import type { CanvasDisplayConnection, CanvasNodeData, ViewportTransform } from "@oc/types/canvas";

export type ViewRect = {
    left: number;
    top: number;
    right: number;
    bottom: number;
};

export function canvasCullViewRect(
    viewport: ViewportTransform,
    viewportSize: { width: number; height: number },
    reduceMediaEffects: boolean,
): ViewRect {
    const padding =
        (reduceMediaEffects
            ? Math.max(240, Math.max(viewportSize.width, viewportSize.height) * 0.4)
            : Math.max(800, Math.max(viewportSize.width, viewportSize.height) * 1.5)) / viewport.k;
    const left = -viewport.x / viewport.k - padding;
    const top = -viewport.y / viewport.k - padding;
    return {
        left,
        top,
        right: left + viewportSize.width / viewport.k + padding * 2,
        bottom: top + viewportSize.height / viewport.k + padding * 2,
    };
}

function nodeTouchesView(node: CanvasNodeData, view: ViewRect): boolean {
    return !(
        node.position.x + node.width <= view.left
        || node.position.x >= view.right
        || node.position.y + node.height <= view.top
        || node.position.y >= view.bottom
    );
}

export function connectionTouchesView(from: CanvasNodeData, to: CanvasNodeData, view: ViewRect): boolean {
    return nodeTouchesView(from, view) || nodeTouchesView(to, view);
}

export function filterDisplayConnections(items: CanvasDisplayConnection[], view: ViewRect): CanvasDisplayConnection[] {
    return items.filter(({ from, to }) => connectionTouchesView(from, to, view));
}

export type CanvasDragPreview = {
    x: number;
    y: number;
    nodeIds: readonly string[];
};

export function applyDragPreviewToDisplayConnections(
    items: CanvasDisplayConnection[],
    preview: CanvasDragPreview | null,
): CanvasDisplayConnection[] {
    if (!preview || preview.nodeIds.length === 0 || (preview.x === 0 && preview.y === 0)) return items;
    const ids = new Set(preview.nodeIds);
    return items.map((item) => {
        const fromHit = ids.has(item.from.id);
        const toHit = ids.has(item.to.id);
        if (!fromHit && !toHit) return item;
        return {
            ...item,
            from: fromHit ? { ...item.from, position: { x: item.from.position.x + preview.x, y: item.from.position.y + preview.y } } : item.from,
            to: toHit ? { ...item.to, position: { x: item.to.position.x + preview.x, y: item.to.position.y + preview.y } } : item.to,
        };
    });
}

export function diffConnectionDrawList(
    prevIds: ReadonlySet<string>,
    next: CanvasDisplayConnection[],
): { add: CanvasDisplayConnection[]; keep: CanvasDisplayConnection[]; remove: string[] } {
    const add: CanvasDisplayConnection[] = [];
    const keep: CanvasDisplayConnection[] = [];
    const nextIds = new Set<string>();
    for (const item of next) {
        nextIds.add(item.connection.id);
        if (prevIds.has(item.connection.id)) keep.push(item);
        else add.push(item);
    }
    const remove: string[] = [];
    for (const id of prevIds) {
        if (!nextIds.has(id)) remove.push(id);
    }
    return { add, keep, remove };
}
