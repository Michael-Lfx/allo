import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";

export type DirectorEnvironmentSource = {
    nodeId: string;
    title: string;
    url: string;
};

function panoramaUrl(node: CanvasNodeData, nodesById: Map<string, CanvasNodeData>, connections: CanvasConnection[]) {
    if (node.metadata?.content) return node.metadata.content;
    const upstream = connections
        .filter((connection) => connection.toNodeId === node.id)
        .map((connection) => nodesById.get(connection.fromNodeId))
        .find((item) => item?.type === CanvasNodeType.Image && item.metadata?.content);
    return upstream?.metadata?.content;
}

/** 导演台片场候选：画布图片 + 已有全景图内容（自身或上游图片）。 */
export function listDirectorEnvironmentSources(nodes: CanvasNodeData[], connections: CanvasConnection[]): DirectorEnvironmentSource[] {
    const nodesById = new Map(nodes.map((node) => [node.id, node]));
    const sources: DirectorEnvironmentSource[] = [];
    for (const node of nodes) {
        if (node.type === CanvasNodeType.Image && node.metadata?.content) {
            sources.push({ nodeId: node.id, title: node.title || node.id, url: node.metadata.content });
            continue;
        }
        if (node.type !== CanvasNodeType.Panorama) continue;
        const url = panoramaUrl(node, nodesById, connections);
        if (!url) continue;
        sources.push({ nodeId: node.id, title: node.title || node.id, url });
    }
    return sources;
}
