import { CANVAS_IMAGE_BATCH_MAX_COUNT } from "@oc/lib/canvas/canvas-generation-count";
import { CanvasNodeType, type CanvasConnection, type CanvasDisplayConnection, type CanvasNodeData, type CanvasNodeMetadata, type Position } from "@oc/types/canvas";

export const IMAGE_DECK_STACK_OFFSET_X = 8;
export const IMAGE_DECK_STACK_OFFSET_Y = 8;
export const IMAGE_DECK_FAN_STEP_X = 52;
export const IMAGE_DECK_FAN_COLLAPSE_MS = 280;
export const IMAGE_DECK_FAN_STAGGER_MS = 24;
export const IMAGE_UPLOAD_GAP = 32;

type NodeLookup = readonly CanvasNodeData[] | ReadonlyMap<string, CanvasNodeData>;

export type ImageDeckUploadItem = {
    id: string;
    title: string;
    size: { width: number; height: number };
    metadata: CanvasNodeMetadata;
};

export type ImageDeckFanMotion = {
    x: number;
    y: number;
    rotate: number;
    index: number;
};

function lookupNode(nodes: NodeLookup, id: string | undefined): CanvasNodeData | undefined {
    if (!id) return undefined;
    if (Array.isArray(nodes)) return nodes.find((node) => node.id === id);
    return (nodes as ReadonlyMap<string, CanvasNodeData>).get(id);
}

export function isImageDeckRoot(node: CanvasNodeData | undefined): boolean {
    return Boolean(
        node
        && node.type === CanvasNodeType.Image
        && node.metadata?.isBatchRoot
        && node.metadata.imageStackLayout === "deck"
        && (node.metadata.batchChildIds?.length || 0) > 0,
    );
}

export function getImageDeckRoot(node: CanvasNodeData | undefined, nodes: NodeLookup): CanvasNodeData | undefined {
    if (!node) return undefined;
    if (isImageDeckRoot(node)) return node;
    const root = lookupNode(nodes, node.metadata?.batchRootId);
    return isImageDeckRoot(root) ? root : undefined;
}

export function isImageDeckMember(node: CanvasNodeData | undefined, nodes: NodeLookup): boolean {
    return Boolean(getImageDeckRoot(node, nodes));
}

/** Generation grid batches keep their own expand/collapse; do not convert them into upload decks. */
export function isGenerationGridCluster(node: CanvasNodeData | undefined, nodes: NodeLookup): boolean {
    if (!node || node.metadata?.imageStackLayout === "deck") return false;
    if (node.metadata?.isBatchRoot && (node.metadata.batchChildIds?.length || 0) > 0) return true;
    const root = lookupNode(nodes, node.metadata?.batchRootId);
    return Boolean(root && root.metadata?.imageStackLayout !== "deck" && root.metadata?.isBatchRoot && (root.metadata.batchChildIds?.length || 0) > 0);
}

export function syncImageDeckAfterRemoval(root: CanvasNodeData, remainingChildIds: string[]): CanvasNodeData {
    if (root.metadata?.imageStackLayout !== "deck" || !root.metadata.isBatchRoot) return root;
    if (!remainingChildIds.length) {
        const metadata = { ...root.metadata };
        delete metadata.isBatchRoot;
        delete metadata.batchChildIds;
        delete metadata.primaryImageId;
        delete metadata.imageStackLayout;
        delete metadata.imageBatchExpanded;
        return { ...root, metadata };
    }
    const keepPrimary = root.metadata.primaryImageId === root.id || remainingChildIds.includes(root.metadata.primaryImageId || "");
    return {
        ...root,
        metadata: {
            ...root.metadata,
            batchChildIds: remainingChildIds,
            primaryImageId: keepPrimary ? root.metadata.primaryImageId || root.id : root.id,
        },
    };
}

export function getImageDeckMembers(root: CanvasNodeData, nodes: NodeLookup): CanvasNodeData[] {
    if (!isImageDeckRoot(root)) return root ? [root] : [];
    const members: CanvasNodeData[] = [root];
    for (const childId of root.metadata?.batchChildIds || []) {
        const child = lookupNode(nodes, childId);
        if (child?.metadata?.batchRootId === root.id) members.push(child);
    }
    return members;
}

export function imageDeckCount(root: CanvasNodeData, nodes?: NodeLookup): number {
    if (!isImageDeckRoot(root)) return 1;
    if (!nodes) return 1 + (root.metadata?.batchChildIds?.length || 0);
    return getImageDeckMembers(root, nodes).length;
}

export function imageDeckChildPosition(rootPosition: Position, childIndex: number): Position {
    return {
        x: rootPosition.x + IMAGE_DECK_STACK_OFFSET_X * (childIndex + 1),
        y: rootPosition.y + IMAGE_DECK_STACK_OFFSET_Y * (childIndex + 1),
    };
}

/** Extra CSS / draw offset from stacked document positions to a book-like row. */
export function imageDeckFanMotion(index: number, count: number): ImageDeckFanMotion {
    if (count <= 1 || index <= 0) return { x: 0, y: 0, rotate: 0, index };
    return {
        x: index * (IMAGE_DECK_FAN_STEP_X - IMAGE_DECK_STACK_OFFSET_X),
        y: -index * IMAGE_DECK_STACK_OFFSET_Y,
        rotate: 0,
        index,
    };
}

export function imageDeckMemberIndex(node: CanvasNodeData, root: CanvasNodeData): number {
    if (node.id === root.id) return 0;
    const index = (root.metadata?.batchChildIds || []).indexOf(node.id);
    return index >= 0 ? index + 1 : 0;
}

export function resolveFannedDeckRootId(input: {
    hoveredNodeId?: string | null;
    selectedNodeIds?: Iterable<string>;
    connectingNodeId?: string | null;
    connectionTargetNodeId?: string | null;
    nodes: NodeLookup;
}): string | null {
    const candidates = [
        input.hoveredNodeId,
        input.connectingNodeId,
        input.connectionTargetNodeId,
        ...(input.selectedNodeIds ? [...input.selectedNodeIds] : []),
    ];
    for (const id of candidates) {
        const node = lookupNode(input.nodes, id || undefined);
        const root = getImageDeckRoot(node, input.nodes);
        if (root) return root.id;
    }
    return null;
}

export function retargetCollapsedDeckNodeId(nodeId: string, nodes: NodeLookup, fannedDeckRootId?: string | null): string {
    const node = lookupNode(nodes, nodeId);
    const root = getImageDeckRoot(node, nodes);
    if (!root) return nodeId;
    if (fannedDeckRootId === root.id) return nodeId;
    return root.id;
}

export function resolveDeckConnection(
    connection: CanvasConnection,
    from: CanvasNodeData,
    to: CanvasNodeData,
    nodes: NodeLookup,
    fannedDeckRootId?: string | null,
): { from: CanvasNodeData; to: CanvasNodeData } | null {
    const fromRoot = getImageDeckRoot(from, nodes);
    const toRoot = getImageDeckRoot(to, nodes);
    const displayFrom = fromRoot && fromRoot.id !== fannedDeckRootId ? fromRoot : from;
    const displayTo = toRoot && toRoot.id !== fannedDeckRootId ? toRoot : to;
    if (displayFrom.id === displayTo.id) return null;
    return { from: displayFrom, to: displayTo };
}

export function applyDeckFanToNode(node: CanvasNodeData, fannedDeckRootId: string | null | undefined, nodes: NodeLookup): CanvasNodeData {
    if (!fannedDeckRootId) return node;
    const root = getImageDeckRoot(node, nodes);
    if (!root || root.id !== fannedDeckRootId) return node;
    const members = getImageDeckMembers(root, nodes);
    const motion = imageDeckFanMotion(imageDeckMemberIndex(node, root), members.length);
    if (!motion.x && !motion.y) return node;
    return {
        ...node,
        position: { x: node.position.x + motion.x, y: node.position.y + motion.y },
    };
}

export function applyDeckDisplayConnections(
    items: CanvasDisplayConnection[],
    nodes: NodeLookup,
    fannedDeckRootId?: string | null,
): CanvasDisplayConnection[] {
    const next: CanvasDisplayConnection[] = [];
    for (const item of items) {
        const resolved = resolveDeckConnection(item.connection, item.from, item.to, nodes, fannedDeckRootId);
        if (!resolved) continue;
        next.push({
            connection: item.connection,
            from: applyDeckFanToNode(resolved.from, fannedDeckRootId, nodes),
            to: applyDeckFanToNode(resolved.to, fannedDeckRootId, nodes),
        });
    }
    return next;
}

/** Expand a connected deck root into mentionable members. Specific child edges stay single. */
export function mentionableDeckNodes(fromNode: CanvasNodeData, nodes: NodeLookup): CanvasNodeData[] {
    const root = getImageDeckRoot(fromNode, nodes);
    if (!root || fromNode.id !== root.id) return [fromNode];
    return getImageDeckMembers(root, nodes);
}

export function expandMentionResourceNodes(nodes: CanvasNodeData[], allNodes: NodeLookup): CanvasNodeData[] {
    const seen = new Set<string>();
    const expanded: CanvasNodeData[] = [];
    for (const node of nodes) {
        for (const member of mentionableDeckNodes(node, allNodes)) {
            if (seen.has(member.id)) continue;
            seen.add(member.id);
            expanded.push(member);
        }
    }
    return expanded;
}

function createStandaloneImageNode(item: ImageDeckUploadItem, position: Position, parentId?: string): CanvasNodeData {
    return {
        id: item.id,
        type: CanvasNodeType.Image,
        title: item.title,
        position,
        width: item.size.width,
        height: item.size.height,
        parentId,
        metadata: {
            ...item.metadata,
            isBatchRoot: undefined,
            batchChildIds: undefined,
            batchRootId: undefined,
            primaryImageId: undefined,
            imageStackLayout: undefined,
            imageBatchExpanded: undefined,
            freeResize: false,
            taskId: undefined,
            errorDetails: undefined,
            generationErrorCode: undefined,
        },
    };
}

function layoutStandaloneImages(items: ImageDeckUploadItem[], start: Position, parentId?: string): CanvasNodeData[] {
    let x = start.x;
    return items.map((item) => {
        const node = createStandaloneImageNode(item, { x, y: start.y }, parentId);
        x += item.size.width + IMAGE_UPLOAD_GAP;
        return node;
    });
}

function replaceNodeMediaFields(node: CanvasNodeData, item: ImageDeckUploadItem, keepSize: boolean): CanvasNodeData {
    return {
        ...node,
        type: CanvasNodeType.Image,
        title: item.title,
        width: keepSize ? node.width : item.size.width,
        height: keepSize ? node.height : item.size.height,
        metadata: {
            ...node.metadata,
            ...item.metadata,
            freeResize: false,
            taskId: undefined,
            errorDetails: undefined,
            generationErrorCode: undefined,
        },
    };
}

export function applyImageDeckUploads(
    nodes: CanvasNodeData[],
    input: {
        items: ImageDeckUploadItem[];
        origin: Position;
        targetNodeId?: string;
        maxCount?: number;
    },
): {
    nodes: CanvasNodeData[];
    rootId: string;
    addedIds: string[];
    truncated: number;
} {
    const maxCount = input.maxCount ?? CANVAS_IMAGE_BATCH_MAX_COUNT;
    const items = input.items.filter((item) => item.id);
    if (!items.length) {
        return { nodes, rootId: input.targetNodeId || "", addedIds: [], truncated: 0 };
    }

    const rawTarget = input.targetNodeId ? nodes.find((node) => node.id === input.targetNodeId) : undefined;
    const target = rawTarget && !isGenerationGridCluster(rawTarget, nodes) ? rawTarget : undefined;
    const taken = items.slice(0, maxCount);
    const truncated = items.length - taken.length;
    const first = taken[0];
    if (!first) return { nodes, rootId: input.targetNodeId || "", addedIds: [], truncated };

    if (!target) {
        const start = {
            x: input.origin.x - first.size.width / 2,
            y: input.origin.y - first.size.height / 2,
        };
        const created = layoutStandaloneImages(taken, start);
        return { nodes: [...nodes, ...created], rootId: created[0].id, addedIds: created.map((node) => node.id), truncated };
    }

    const replaced = replaceNodeMediaFields(target, first, true);
    const created = layoutStandaloneImages(
        taken.slice(1),
        { x: replaced.position.x + replaced.width + IMAGE_UPLOAD_GAP, y: replaced.position.y },
        target.parentId,
    );
    return {
        nodes: [...nodes.map((node) => (node.id === target.id ? replaced : node)), ...created],
        rootId: replaced.id,
        addedIds: created.map((node) => node.id),
        truncated,
    };
}

export function collectImageDeckCopyIds(targetIds: Iterable<string>, nodes: readonly CanvasNodeData[]): Set<string> {
    const copyIds = new Set(targetIds);
    const nodeById = new Map(nodes.map((node) => [node.id, node]));
    for (const id of [...copyIds]) {
        const node = nodeById.get(id);
        const root = getImageDeckRoot(node, nodeById);
        if (root && copyIds.has(root.id)) {
            for (const member of getImageDeckMembers(root, nodeById)) copyIds.add(member.id);
        }
    }
    return copyIds;
}

export function preserveCopiedImageDeck(
    node: CanvasNodeData,
    idMap: ReadonlyMap<string, string>,
    copiedSourceIds: ReadonlySet<string> | undefined,
): Partial<CanvasNodeMetadata> | null {
    if (node.metadata?.imageStackLayout !== "deck" || !copiedSourceIds) return null;
    if (node.metadata.isBatchRoot) {
        const childIds = node.metadata.batchChildIds || [];
        if (!childIds.length || childIds.some((childId) => !copiedSourceIds.has(childId))) return null;
        const remapped = childIds.map((childId) => idMap.get(childId)).filter((childId): childId is string => Boolean(childId));
        if (remapped.length !== childIds.length) return null;
        const primarySource = node.metadata.primaryImageId && copiedSourceIds.has(node.metadata.primaryImageId) ? node.metadata.primaryImageId : node.id;
        return {
            isBatchRoot: true,
            imageStackLayout: "deck",
            batchChildIds: remapped,
            primaryImageId: idMap.get(primarySource) || idMap.get(node.id),
            imageBatchExpanded: false,
            batchRootId: undefined,
        };
    }
    const rootId = node.metadata.batchRootId;
    if (rootId && copiedSourceIds.has(rootId) && idMap.get(rootId)) {
        return {
            batchRootId: idMap.get(rootId),
            imageStackLayout: "deck",
            isBatchRoot: undefined,
            batchChildIds: undefined,
            primaryImageId: undefined,
            imageBatchExpanded: false,
        };
    }
    return null;
}

export function deckPrimaryNodeId(root: CanvasNodeData): string {
    return root.metadata?.primaryImageId || root.id;
}
