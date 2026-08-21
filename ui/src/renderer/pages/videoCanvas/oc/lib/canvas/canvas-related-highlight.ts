type RelatedConnection = {
    id: string;
    fromNodeId: string;
    toNodeId: string;
};

export function canvasActiveNodeId(hoveredNodeId: string | null, selectedNodeIds: ReadonlySet<string>): string | null {
    if (selectedNodeIds.size > 1) return null;
    if (hoveredNodeId) return hoveredNodeId;
    if (selectedNodeIds.size === 1) return [...selectedNodeIds][0] ?? null;
    return null;
}

export function canvasRelatedHighlight(activeNodeId: string | null, connections: readonly RelatedConnection[]): {
    nodeIds: Set<string>;
    connectionIds: Set<string>;
} {
    const nodeIds = new Set<string>();
    const connectionIds = new Set<string>();
    if (!activeNodeId) return { nodeIds, connectionIds };
    nodeIds.add(activeNodeId);
    connections.forEach((connection) => {
        if (connection.fromNodeId !== activeNodeId && connection.toNodeId !== activeNodeId) return;
        connectionIds.add(connection.id);
        nodeIds.add(connection.fromNodeId);
        nodeIds.add(connection.toNodeId);
    });
    return { nodeIds, connectionIds };
}
