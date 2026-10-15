import React, { useCallback, useEffect, useMemo, useState, type CSSProperties, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode, type RefObject } from "react";
import { Link2 } from "lucide-react";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";

import { ConnectionPath } from "@oc/components/canvas/canvas-connections";
import { CanvasFrameNode } from "@oc/components/canvas/canvas-frame-node";
import { CanvasNode } from "@oc/components/canvas/canvas-node";
import type { CanvasBatchConnectionPreview } from "@oc/lib/canvas/canvas-batch-connection";
import { applyDragPreviewToDisplayConnections } from "@oc/lib/canvas/canvas-connection-draw-list";
import { applyDeckDisplayConnections, deckPrimaryNodeId, getImageDeckRoot, imageDeckCount, imageDeckFanMotion, imageDeckMemberIndex, IMAGE_DECK_FAN_COLLAPSE_MS, resolveFannedDeckRootId } from "@oc/lib/canvas/canvas-image-stack";
import { isFrameNode } from "@oc/lib/canvas/canvas-frame";
import { bringCanvasNodeToFront, sortCanvasNodesByStackOrder } from "@oc/lib/canvas/canvas-node-stack-order";
import { resolveActiveCanvasMediaNodeId } from "@oc/lib/canvas/canvas-performance-mode";
import { canvasActiveNodeId, canvasRelatedHighlight } from "@oc/lib/canvas/canvas-related-highlight";
import type { CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import { useCanvasInteractionStore } from "@oc/stores/canvas/use-canvas-interaction-store";
import type { CanvasConnection, CanvasDisplayConnection, CanvasNodeData, Position } from "@oc/types/canvas";

type NodeBounds = { left: number; top: number; width: number; height: number; count: number } | null;

type CanvasProjectWorldLayersProps = {
    projectId: string;
    connectionLayerBounds: { left: number; top: number; width: number; height: number };
    displayConnections: CanvasDisplayConnection[];
    selectedConnectionId: string | null;
    connections: CanvasConnection[];
    scriptScrollTopById: Record<string, number>;
    nodeById: Map<string, CanvasNodeData>;
    visibleNodes: CanvasNodeData[];
    frameChildrenById: Map<string, CanvasNodeData[]>;
    selectedNodeIds: Set<string>;
    batchChildCountById: Map<string, number>;
    collapsingBatchIds: Set<string>;
    openingBatchIds: Set<string>;
    batchMotionById: Map<string, { x: number; y: number; index: number }>;
    showImageInfo: boolean;
    reduceMediaEffects: boolean;
    resourceReferenceByNodeId: Map<string, CanvasResourceReference>;
    mentionReferencesByNodeId: Map<string, CanvasResourceReference[]>;
    mediaEffectsDisabledNodeId?: string | null;
    selectedNodeBounds: NodeBounds;
    batchSourceNodeIds: string[];
    batchConnectionPreview: CanvasBatchConnectionPreview | null;
    selectionBoundsElementRef: RefObject<HTMLDivElement | null>;
    renderCanvasNodeContent: (node: CanvasNodeData) => ReactNode;
    onConnectionSelect: (connectionId: string) => void;
    onConnectionContextMenu: (event: ReactMouseEvent<SVGPathElement>, connectionId: string) => void;
    onNodeMouseDown: (event: ReactMouseEvent, nodeId: string) => void;
    onNodeHoverStart: (nodeId: string) => void;
    onNodeHoverEnd: (nodeId: string) => void;
    onConnectStart: (event: ReactPointerEvent, nodeId: string, handleType: "source" | "target", handleId?: string, anchorRatio?: number) => void;
    onNodeResize: (nodeId: string, width: number, height: number, position?: Position, options?: { markManual?: boolean }) => void;
    onToggleFrame: (nodeId: string) => void;
    onNodeTitleChange: (nodeId: string, title: string) => void;
    onNodeContextMenu: (event: ReactMouseEvent, nodeId: string) => void;
    onNodeContentChange: (nodeId: string, content: string) => void;
    onToggleBatch: (nodeId: string) => void;
    onSetBatchPrimary: (node: CanvasNodeData) => void;
    onRetry: (node: CanvasNodeData) => void;
    onReloadResource: (node: CanvasNodeData) => void;
    onCancelTask: (node: CanvasNodeData) => void;
    onOpenTaskDetails: (node: CanvasNodeData) => void;
    onOpenVersions: (node: CanvasNodeData) => void;
    onViewImage: (node: CanvasNodeData) => void;
    onOpenTextEditor: (node: CanvasNodeData) => void;
    onOpenDirector: (node: CanvasNodeData) => void;
    onOpenDrawing: (node: CanvasNodeData) => void;
    onStartBatchConnection: (event: ReactPointerEvent, sourceNodeIds: string[]) => void;
};

const EMPTY_RESOURCE_REFERENCES: CanvasResourceReference[] = [];
const EMPTY_CANVAS_NODES: CanvasNodeData[] = [];

function deckPaintProps(node: CanvasNodeData, nodeById: Map<string, CanvasNodeData>, fannedDeckRootId: string | null) {
    const deckRoot = getImageDeckRoot(node, nodeById);
    if (!deckRoot) {
        return { deckFanned: false, deckIndex: 0, deckCount: 0, deckFanMotion: undefined as ReturnType<typeof imageDeckFanMotion> | undefined, hideConnectionRails: false, deckPrimary: false };
    }
    const deckCount = imageDeckCount(deckRoot, nodeById);
    const deckIndex = imageDeckMemberIndex(node, deckRoot);
    const deckFanned = fannedDeckRootId === deckRoot.id;
    return {
        deckFanned,
        deckIndex,
        deckCount,
        deckFanMotion: imageDeckFanMotion(deckIndex, deckCount),
        hideConnectionRails: !deckFanned && node.id !== deckRoot.id,
        deckPrimary: deckPrimaryNodeId(deckRoot) === node.id,
    };
}

export function HideWhileNodeDragging({ children }: { children: ReactNode }) {
    const isNodeDragging = useCanvasInteractionStore((state) => state.isNodeDragging);
    if (isNodeDragging) return null;
    return children;
}

export function HideWhileSelectionBox({ children }: { children: ReactNode }) {
    const selectionBox = useCanvasInteractionStore((state) => state.selectionBox);
    if (selectionBox) return null;
    return children;
}

export const CanvasProjectWorldLayers = React.memo(function CanvasProjectWorldLayers(props: CanvasProjectWorldLayersProps) {
    const hoveredNodeId = useCanvasInteractionStore((state) => state.hoveredNodeId);
    const dragPreview = useCanvasInteractionStore((state) => state.dragPreview);
    const frameDropTargetId = useCanvasInteractionStore((state) => state.frameDropTargetId);
    const isNodeDragging = useCanvasInteractionStore((state) => state.isNodeDragging);
    const connectingParams = useCanvasInteractionStore((state) => state.connectingParams);
    const connectionTargetNodeId = useCanvasInteractionStore((state) => state.connectionTargetNodeId);
    const setFannedDeckRootId = useCanvasInteractionStore((state) => state.setFannedDeckRootId);
    const selectionBox = useCanvasInteractionStore((state) => state.selectionBox);
    const [nodeStackOrder, setNodeStackOrder] = useState<string[]>([]);
    const immediateFanRootId = useMemo(
        () => resolveFannedDeckRootId({
            hoveredNodeId,
            selectedNodeIds: props.selectedNodeIds,
            connectingNodeId: connectingParams?.nodeId,
            connectionTargetNodeId,
            nodes: props.nodeById,
        }),
        [connectingParams?.nodeId, connectionTargetNodeId, hoveredNodeId, props.nodeById, props.selectedNodeIds],
    );
    const [fannedDeckRootId, setLocalFannedDeckRootId] = useState<string | null>(null);
    useEffect(() => {
        if (immediateFanRootId) {
            setLocalFannedDeckRootId(immediateFanRootId);
            setFannedDeckRootId(immediateFanRootId);
            return;
        }
        const timer = window.setTimeout(() => {
            setLocalFannedDeckRootId(null);
            setFannedDeckRootId(null);
        }, IMAGE_DECK_FAN_COLLAPSE_MS);
        return () => window.clearTimeout(timer);
    }, [immediateFanRootId, setFannedDeckRootId]);
    const paintedNodes = useMemo(() => sortCanvasNodesByStackOrder(props.visibleNodes, nodeStackOrder), [nodeStackOrder, props.visibleNodes]);
    const mediaActiveNodeId = resolveActiveCanvasMediaNodeId(props.selectedNodeIds, props.nodeById);
    const bringNodeToFront = useCallback((nodeId: string) => {
        setNodeStackOrder((order) => bringCanvasNodeToFront(order, nodeId));
    }, []);
    const handleNodeMouseDown = useCallback((event: ReactMouseEvent, nodeId: string) => {
        bringNodeToFront(nodeId);
        props.onNodeMouseDown(event, nodeId);
    }, [bringNodeToFront, props.onNodeMouseDown]);
    const handleNodeHoverStart = useCallback((nodeId: string) => {
        bringNodeToFront(nodeId);
        props.onNodeHoverStart(nodeId);
    }, [bringNodeToFront, props.onNodeHoverStart]);
    const activeNodeId = canvasActiveNodeId(hoveredNodeId, props.selectedNodeIds);
    const relatedHighlight = useMemo(
        () => canvasRelatedHighlight(activeNodeId, props.connections),
        [activeNodeId, props.connections],
    );
    const dragNodeIds = useMemo(() => new Set(dragPreview?.nodeIds ?? []), [dragPreview]);
    const displayConnections = useMemo(
        () => applyDeckDisplayConnections(applyDragPreviewToDisplayConnections(props.displayConnections, dragPreview), props.nodeById, fannedDeckRootId),
        [dragPreview, fannedDeckRootId, props.displayConnections, props.nodeById],
    );

    return (
        <>
            <svg
                className="absolute overflow-visible"
                viewBox={`${props.connectionLayerBounds.left} ${props.connectionLayerBounds.top} ${props.connectionLayerBounds.width} ${props.connectionLayerBounds.height}`}
                style={{ left: props.connectionLayerBounds.left, top: props.connectionLayerBounds.top, width: props.connectionLayerBounds.width, height: props.connectionLayerBounds.height, pointerEvents: "none", zIndex: 0 }}
            >
                {displayConnections.map(({ connection, from, to }) => (
                    <ConnectionPath
                        key={connection.id}
                        connection={connection}
                        from={from}
                        to={to}
                        fromScrollTop={props.scriptScrollTopById[from.id] || 0}
                        toScrollTop={props.scriptScrollTopById[to.id] || 0}
                        active={props.selectedConnectionId === connection.id || relatedHighlight.connectionIds.has(connection.id)}
                        visualMode="hover-only"
                        hideVisual={isNodeDragging}
                        onSelect={() => props.onConnectionSelect(connection.id)}
                        onContextMenu={(event) => props.onConnectionContextMenu(event, connection.id)}
                    />
                ))}
            </svg>

            {paintedNodes.map((node) => {
                if (isFrameNode(node)) {
                    return (
                        <CanvasFrameNode
                            key={node.id}
                            data={node}
                            dragOffset={dragNodeIds.has(node.id) && dragPreview ? dragPreview : undefined}
                            childNodes={props.frameChildrenById.get(node.id) || EMPTY_CANVAS_NODES}
                            isSelected={props.selectedNodeIds.has(node.id)}
                            isDropTarget={frameDropTargetId === node.id}
                            onMouseDown={handleNodeMouseDown}
                            onResize={props.onNodeResize}
                            onToggleCollapsed={props.onToggleFrame}
                            onTitleChange={props.onNodeTitleChange}
                            onContextMenu={props.onNodeContextMenu}
                        />
                    );
                }
                const deck = deckPaintProps(node, props.nodeById, fannedDeckRootId);
                return (
                    <CanvasNode
                        key={node.id}
                        data={node}
                        dragOffset={dragNodeIds.has(node.id) && dragPreview ? dragPreview : undefined}
                        isSelected={props.selectedNodeIds.has(node.id)}
                        isRelated={relatedHighlight.nodeIds.has(node.id)}
                        isFocusRelated={activeNodeId === node.id}
                        isConnectionTarget={connectionTargetNodeId === node.id || props.batchConnectionPreview?.targetNodeId === node.id}
                        isConnecting={Boolean(connectingParams)}
                        forceInputVisible={Boolean(props.batchConnectionPreview)}
                        batchCount={props.batchChildCountById.get(node.id) || 0}
                        batchExpanded={Boolean(node.metadata?.imageBatchExpanded)}
                        batchClosing={Boolean(node.metadata?.batchRootId && props.collapsingBatchIds.has(node.metadata.batchRootId) && node.metadata?.imageStackLayout !== "deck")}
                        batchOpening={props.openingBatchIds.has(node.id)}
                        batchRecovering={props.collapsingBatchIds.has(node.id)}
                        batchPrimary={deck.deckCount > 0 ? deck.deckPrimary : Boolean(node.metadata?.batchRootId && props.nodeById.get(node.metadata.batchRootId)?.metadata?.primaryImageId === node.id)}
                        batchMotion={node.metadata?.imageStackLayout === "deck" ? undefined : props.batchMotionById.get(node.id)}
                        deckFanned={deck.deckFanned}
                        deckIndex={deck.deckIndex}
                        deckCount={deck.deckCount}
                        deckFanMotion={deck.deckFanMotion}
                        hideConnectionRails={deck.hideConnectionRails}
                        mediaActive={mediaActiveNodeId === node.id}
                        hydrateMediaPreview={!props.reduceMediaEffects}
                        showImageInfo={props.showImageInfo}
                        reduceMediaEffects={props.reduceMediaEffects || isNodeDragging || props.mediaEffectsDisabledNodeId === node.id}
                        resourceLabel={props.resourceReferenceByNodeId.get(node.id)}
                        mentionReferences={props.mentionReferencesByNodeId.get(node.id) || EMPTY_RESOURCE_REFERENCES}
                        renderNodeContent={props.renderCanvasNodeContent}
                        drawingProjectId={props.projectId}
                        onMouseDown={handleNodeMouseDown}
                        onHoverStart={handleNodeHoverStart}
                        onHoverEnd={props.onNodeHoverEnd}
                        onConnectStart={props.onConnectStart}
                        onResize={props.onNodeResize}
                        onTitleChange={props.onNodeTitleChange}
                        onContentChange={props.onNodeContentChange}
                        onToggleBatch={props.onToggleBatch}
                        onSetBatchPrimary={props.onSetBatchPrimary}
                        onRetry={props.onRetry}
                        onReloadResource={props.onReloadResource}
                        onCancelTask={props.onCancelTask}
                        onOpenTaskDetails={props.onOpenTaskDetails}
                        onOpenVersions={props.onOpenVersions}
                        onViewImage={props.onViewImage}
                        onOpenTextEditor={props.onOpenTextEditor}
                        onOpenDirector={props.onOpenDirector}
                        onOpenDrawing={props.onOpenDrawing}
                        onContextMenu={props.onNodeContextMenu}
                    />
                );
            })}

            {props.selectedNodeBounds && !selectionBox && !isNodeDragging ? (
                <div
                    ref={props.selectionBoundsElementRef}
                    className="pointer-events-none absolute z-[var(--z-panel-floating)] rounded-xl"
                    style={{
                        left: `calc(${props.selectedNodeBounds.left}px - 12px / var(--canvas-committed-scale, 1))`,
                        top: `calc(${props.selectedNodeBounds.top}px - 12px / var(--canvas-committed-scale, 1))`,
                        width: `calc(${props.selectedNodeBounds.width}px + 24px / var(--canvas-committed-scale, 1))`,
                        height: `calc(${props.selectedNodeBounds.height}px + 24px / var(--canvas-committed-scale, 1))`,
                    }}
                >
                    {props.batchSourceNodeIds.length > 0 ? (
                        <BatchConnectionHandle
                            count={props.batchSourceNodeIds.length}
                            active={Boolean(props.batchConnectionPreview)}
                            onPointerDown={(event) => props.onStartBatchConnection(event, props.batchSourceNodeIds)}
                        />
                    ) : null}
                </div>
            ) : null}
        </>
    );
});

function BatchConnectionHandle({ count, active, onPointerDown }: { count: number; active: boolean; onPointerDown: (event: ReactPointerEvent) => void }) {
    const buttonStyle: CSSProperties = {
        right: "calc(-18px * var(--canvas-live-inverse-scale, 1))",
        top: "50%",
        width: "calc(30px * var(--canvas-live-inverse-scale, 1))",
        height: "calc(30px * var(--canvas-live-inverse-scale, 1))",
        background: active ? "var(--workspace-accent)" : "var(--workspace-surface-strong)",
        borderColor: active ? "var(--workspace-accent)" : "var(--workspace-border)",
        color: active ? "var(--workspace-accent-foreground)" : "var(--foreground)",
    };
    return (
        <button
            type="button"
            data-canvas-no-zoom
            className="pointer-events-auto absolute grid -translate-y-1/2 translate-x-1/2 place-items-center rounded-full border shadow-md transition hover:scale-110 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2"
            style={buttonStyle}
            title={canvasT("videoCanvas.worldLayers.batchConnectTitle", "批量连接 {{count}} 个节点", { count })}
            aria-label={canvasT("videoCanvas.worldLayers.batchConnectAria", "批量连接 {{count}} 个节点", { count })}
            onPointerDown={onPointerDown}
        >
            <Link2 style={{ width: "calc(14px * var(--canvas-live-inverse-scale, 1))", height: "calc(14px * var(--canvas-live-inverse-scale, 1))" }} strokeWidth={2} />
            <span className="sr-only">{canvasT("videoCanvas.worldLayers.batchConnectSr", "连接 {{count}} 个节点", { count })}</span>
        </button>
    );
}
