import type { CanvasNodeData, ViewportTransform } from "@oc/types/canvas";

function clamp(value: number, min: number, max: number) {
    return Math.min(Math.max(value, min), max);
}

export function getNodePanelPosition(node: CanvasNodeData, viewport: ViewportTransform, viewportSize: { width: number; height: number }, panelWidth: number) {
    const gap = 10;
    const margin = 12;
    const nodeCenterX = viewport.x + (node.position.x + node.width / 2) * viewport.k;
    const nodeBottom = viewport.y + (node.position.y + node.height) * viewport.k;
    const maxLeft = Math.max(margin, viewportSize.width - panelWidth - margin);
    return {
        left: clamp(nodeCenterX - panelWidth / 2, margin, maxLeft),
        top: nodeBottom + gap,
    };
}
