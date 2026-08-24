import { useLayoutEffect, useMemo, useRef, type RefObject } from "react";
import { Group, Leafer, Path, Rect } from "leafer-ui";

import { activeConnectionPath, canvasConnectionPath } from "@oc/components/canvas/canvas-connections";
import type { CanvasBatchConnectionPreview } from "@oc/lib/canvas/canvas-batch-connection";
import { applyDragPreviewToDisplayConnections, diffConnectionDrawList } from "@oc/lib/canvas/canvas-connection-draw-list";
import { subscribeCanvasGraphicsViewportPreview, subscribeCanvasSelectionPreview } from "@oc/lib/canvas/canvas-live-viewport";
import { calculateCanvasPreviewTransform, sameCanvasViewport, shouldRebaseCanvasRaster } from "@oc/lib/canvas/canvas-leafer-viewport";
import { canvasActiveNodeId, canvasRelatedHighlight } from "@oc/lib/canvas/canvas-related-highlight";
import { useCanvasInteractionStore } from "@oc/stores/canvas/use-canvas-interaction-store";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasConnection, CanvasDisplayConnection, CanvasNodeData, ConnectionHandle, Position, SelectionBox, ViewportTransform } from "@oc/types/canvas";

type NodeBounds = { left: number; top: number; width: number; height: number; count: number } | null;

type CanvasLeaferGraphicsLayerProps = {
    containerRef: RefObject<HTMLDivElement | null>;
    viewport: ViewportTransform;
    theme: CanvasTheme;
    displayConnections: CanvasDisplayConnection[];
    selectedConnectionId: string | null;
    connections: CanvasConnection[];
    selectedNodeIds: Set<string>;
    scriptScrollTopById: Record<string, number>;
    nodeById: Map<string, CanvasNodeData>;
    selectedNodeBounds: NodeBounds;
    batchConnectionPreview: CanvasBatchConnectionPreview | null;
};

type ResolvedLeaferProps = CanvasLeaferGraphicsLayerProps & {
    relatedConnectionIds: Set<string>;
    alignmentGuides: { vertical?: number; horizontal?: number };
    displayConnections: CanvasDisplayConnection[];
    connectingParams: ConnectionHandle | null;
    mouseWorld: Position;
    connectionTargetNodeId: string | null;
    connectionTargetAnchorRatio: number | undefined;
    selectionBox: SelectionBox | null;
};

type LeaferScene = {
    leafer: Leafer;
    world: Group;
    host: HTMLDivElement;
};

type UnderlayScene = LeaferScene & {
    connections: Group;
};

type OverlayScene = LeaferScene & {
    selection: Rect;
    selectionBounds: Rect;
    guides: Path;
    draft: Path;
    batchDrafts: Group;
};

export function CanvasLeaferGraphicsLayer(props: CanvasLeaferGraphicsLayerProps) {
    const underlayHostRef = useRef<HTMLDivElement>(null);
    const overlayHostRef = useRef<HTMLDivElement>(null);
    const underlayRef = useRef<UnderlayScene | null>(null);
    const overlayRef = useRef<OverlayScene | null>(null);
    const connectionPathsRef = useRef(new Map<string, Path>());
    const viewportRef = useRef(props.viewport);
    const rasterViewportRef = useRef(props.viewport);
    const hoveredNodeId = useCanvasInteractionStore((state) => state.hoveredNodeId);
    const dragPreview = useCanvasInteractionStore((state) => state.dragPreview);
    const alignmentGuides = useCanvasInteractionStore((state) => state.alignmentGuides);
    const connectingParams = useCanvasInteractionStore((state) => state.connectingParams);
    const mouseWorld = useCanvasInteractionStore((state) => state.mouseWorld);
    const connectionTargetNodeId = useCanvasInteractionStore((state) => state.connectionTargetNodeId);
    const connectionTargetAnchorRatio = useCanvasInteractionStore((state) => state.connectionTargetAnchorRatio);
    const selectionBox = useCanvasInteractionStore((state) => state.selectionBox);
    const relatedHighlight = useMemo(
        () => canvasRelatedHighlight(canvasActiveNodeId(hoveredNodeId, props.selectedNodeIds), props.connections),
        [hoveredNodeId, props.connections, props.selectedNodeIds],
    );
    const displayConnections = useMemo(
        () => applyDragPreviewToDisplayConnections(props.displayConnections, dragPreview),
        [dragPreview, props.displayConnections],
    );
    const resolvedProps: ResolvedLeaferProps = {
        ...props,
        displayConnections,
        relatedConnectionIds: relatedHighlight.connectionIds,
        alignmentGuides,
        connectingParams,
        mouseWorld,
        connectionTargetNodeId,
        connectionTargetAnchorRatio,
        selectionBox,
    };
    const propsRef = useRef(resolvedProps);
    propsRef.current = resolvedProps;

    useLayoutEffect(() => {
        const underlayHost = underlayHostRef.current;
        const overlayHost = overlayHostRef.current;
        // 子组件 layout effect 可能早于父层 ref 对外可见，host 的直接父元素才是此刻最可靠的画布容器。
        const container = (props.containerRef.current || underlayHost?.parentElement) as HTMLDivElement | null;
        if (!underlayHost || !overlayHost || !container) return;

        const underlay = createUnderlayScene(underlayHost);
        const overlay = createOverlayScene(overlayHost);
        underlayRef.current = underlay;
        overlayRef.current = overlay;

        const resize = () => {
            const rect = container.getBoundingClientRect();
            const size = { width: Math.max(1, rect.width), height: Math.max(1, rect.height), pixelRatio: canvasPixelRatio() };
            underlay.leafer.resize(size);
            overlay.leafer.resize(size);
            syncViewport(rasterViewportRef.current, size.width, size.height, underlay, overlay, propsRef.current);
            if (isViewportPreview(container, viewportRef.current, rasterViewportRef.current)) {
                applyScenePreview(viewportRef.current, rasterViewportRef.current, underlay, overlay);
            }
        };
        const resizeObserver = new ResizeObserver(resize);
        resizeObserver.observe(container);
        window.addEventListener("resize", resize);
        const unsubscribe = subscribeCanvasGraphicsViewportPreview(container, (next) => {
            viewportRef.current = next;
            const rect = container.getBoundingClientRect();
            if (isViewportPreview(container, next, rasterViewportRef.current)) {
                if (shouldRebaseCanvasRaster(next, rasterViewportRef.current)) {
                    syncViewport(next, rect.width, rect.height, underlay, overlay, propsRef.current);
                    rasterViewportRef.current = next;
                    forceSceneRender(underlay, overlay);
                    resetScenePreview(underlay, overlay);
                    return;
                }
                applyScenePreview(next, rasterViewportRef.current, underlay, overlay);
                return;
            }
            resetScenePreview(underlay, overlay);
            if (sameCanvasViewport(next, rasterViewportRef.current)) return;
            syncViewport(next, rect.width, rect.height, underlay, overlay, propsRef.current);
            rasterViewportRef.current = next;
        });
        const unsubscribeSelection = subscribeCanvasSelectionPreview(container, (selection) => {
            syncSelection(overlay.selection, selection, propsRef.current.theme);
        });
        resize();
        syncConnectionPaths(underlay, propsRef.current, connectionPathsRef.current);

        return () => {
            unsubscribe();
            unsubscribeSelection();
            resizeObserver.disconnect();
            window.removeEventListener("resize", resize);
            connectionPathsRef.current.clear();
            underlay.leafer.destroy(true);
            overlay.leafer.destroy(true);
            underlayRef.current = null;
            overlayRef.current = null;
        };
    }, [props.containerRef]);

    useLayoutEffect(() => {
        const underlay = underlayRef.current;
        if (!underlay) return;
        syncConnectionPaths(underlay, resolvedProps, connectionPathsRef.current);
    }, [displayConnections, props.scriptScrollTopById, props.selectedConnectionId, props.theme, relatedHighlight.connectionIds]);

    useLayoutEffect(() => {
        const overlay = overlayRef.current;
        if (!overlay) return;
        syncOverlayContent(overlay, resolvedProps, viewportRef.current.k);
    }, [connectingParams, connectionTargetAnchorRatio, connectionTargetNodeId, mouseWorld, props.batchConnectionPreview, props.nodeById, props.scriptScrollTopById, props.selectedNodeBounds, selectionBox, props.theme]);

    useLayoutEffect(() => {
        const underlay = underlayRef.current;
        const overlay = overlayRef.current;
        const container = props.containerRef.current;
        if (!underlay || !overlay || !container) return;
        viewportRef.current = props.viewport;
        const rect = container.getBoundingClientRect();
        const hadPreview = hasScenePreview(underlay, overlay);
        if (hadPreview || !sameCanvasViewport(props.viewport, rasterViewportRef.current)) {
            syncViewport(props.viewport, rect.width, rect.height, underlay, overlay, resolvedProps);
        }
        rasterViewportRef.current = props.viewport;
        // 新视口先同步到真实 DPR backing store，再撤销交互期的合成变换，避免出现跳帧。
        if (hadPreview) forceSceneRender(underlay, overlay);
        resetScenePreview(underlay, overlay);
    }, [props.containerRef, props.viewport]);

    useLayoutEffect(() => {
        const underlay = underlayRef.current;
        const overlay = overlayRef.current;
        const container = props.containerRef.current;
        if (!underlay || !overlay || !container) return;
        const rect = container.getBoundingClientRect();
        syncViewport(rasterViewportRef.current, rect.width, rect.height, underlay, overlay, resolvedProps);
        if (isViewportPreview(container, viewportRef.current, rasterViewportRef.current)) {
            applyScenePreview(viewportRef.current, rasterViewportRef.current, underlay, overlay);
        }
    }, [alignmentGuides, props.containerRef, props.theme]);

    return (
        <>
            <div ref={underlayHostRef} data-canvas-leafer-underlay className="pointer-events-none absolute inset-0 z-0 overflow-hidden" aria-hidden />
            <div ref={overlayHostRef} data-canvas-leafer-overlay className="pointer-events-none absolute inset-0 z-[var(--z-canvas-overlay)] overflow-hidden" aria-hidden />
        </>
    );
}

function createUnderlayScene(host: HTMLDivElement): UnderlayScene {
    const leafer = new Leafer({ view: host, width: 1, height: 1, pixelRatio: canvasPixelRatio(), fill: "transparent", hittable: false, smooth: true });
    const world = new Group({ hittable: false });
    const connections = new Group({ hittable: false });
    world.add(connections);
    leafer.add(world);
    return { leafer, world, host, connections };
}

function createOverlayScene(host: HTMLDivElement): OverlayScene {
    const leafer = new Leafer({ view: host, width: 1, height: 1, pixelRatio: canvasPixelRatio(), fill: "transparent", hittable: false, smooth: true });
    const world = new Group({ hittable: false });
    const selection = new Rect({ visible: false, hittable: false });
    const selectionBounds = new Rect({ visible: false, hittable: false, fill: "transparent" });
    const guides = new Path({ visible: false, hittable: false });
    const draft = new Path({ visible: false, hittable: false });
    const batchDrafts = new Group({ visible: false, hittable: false });
    world.add(selection);
    world.add(selectionBounds);
    world.add(guides);
    world.add(draft);
    world.add(batchDrafts);
    leafer.add(world);
    return { leafer, world, host, selection, selectionBounds, guides, draft, batchDrafts };
}

type ConnectionPathAppearance = {
    pathD: string;
    emphasized: boolean;
    stroke: string;
    strokeWidth: number;
    opacity: number;
};

const connectionPathAppearance = new WeakMap<Path, ConnectionPathAppearance>();

function connectionAppearance(item: CanvasDisplayConnection, props: ResolvedLeaferProps): ConnectionPathAppearance {
    const emphasized = props.selectedConnectionId === item.connection.id || props.relatedConnectionIds.has(item.connection.id);
    return {
        pathD: canvasConnectionPath(item.connection, item.from, item.to, props.scriptScrollTopById[item.from.id] || 0, props.scriptScrollTopById[item.to.id] || 0).pathD,
        emphasized,
        stroke: emphasized ? props.theme.accent.primary : props.theme.node.muted,
        strokeWidth: emphasized ? 1.6 : 1,
        opacity: emphasized ? 0.52 : 0.24,
    };
}

function sameConnectionAppearance(previous: ConnectionPathAppearance | undefined, next: ConnectionPathAppearance) {
    return previous !== undefined && previous.pathD === next.pathD && previous.emphasized === next.emphasized && previous.stroke === next.stroke;
}

function syncConnectionPaths(scene: UnderlayScene, props: ResolvedLeaferProps, paths: Map<string, Path>) {
    const { add, keep, remove } = diffConnectionDrawList(new Set(paths.keys()), props.displayConnections);

    for (const id of remove) {
        const path = paths.get(id);
        if (!path) continue;
        path.destroy();
        paths.delete(id);
    }

    for (const item of add) {
        const appearance = connectionAppearance(item, props);
        const path = new Path({
            path: appearance.pathD,
            stroke: appearance.stroke,
            strokeWidth: appearance.strokeWidth,
            strokeScaleFixed: true,
            strokeCap: "round",
            opacity: appearance.opacity,
            hittable: false,
        });
        scene.connections.add(path);
        paths.set(item.connection.id, path);
        connectionPathAppearance.set(path, appearance);
    }

    for (const item of keep) {
        const path = paths.get(item.connection.id);
        if (!path) continue;
        const appearance = connectionAppearance(item, props);
        if (sameConnectionAppearance(connectionPathAppearance.get(path), appearance)) continue;
        path.set({
            path: appearance.pathD,
            stroke: appearance.stroke,
            strokeWidth: appearance.strokeWidth,
            opacity: appearance.opacity,
        });
        connectionPathAppearance.set(path, appearance);
    }
}

function syncOverlayContent(scene: OverlayScene, props: ResolvedLeaferProps, viewportScale: number) {
    const selection = props.selectionBox;
    scene.selection.visible = Boolean(selection);
    if (selection) {
        syncSelection(scene.selection, selection, props.theme);
    }

    const bounds = props.selectedNodeBounds;
    scene.selectionBounds.visible = Boolean(bounds && !selection);
    if (bounds && !selection) {
        syncSelectionBounds(scene.selectionBounds, bounds, viewportScale);
        scene.selectionBounds.stroke = props.theme.accent.primary;
    }

    const connecting = props.connectingParams;
    scene.draft.visible = Boolean(connecting);
    if (connecting) {
        scene.draft.set({
            path: activeConnectionPath(
                props.nodeById.get(connecting.nodeId),
                connecting,
                props.mouseWorld,
                props.connectionTargetNodeId ? props.nodeById.get(props.connectionTargetNodeId) : undefined,
                props.scriptScrollTopById[connecting.nodeId] || 0,
                props.connectionTargetAnchorRatio,
            ),
            stroke: props.theme.accent.primary,
            strokeCap: "round",
            opacity: 0.72,
        });
    }

    for (const child of [...scene.batchDrafts.children]) child.destroy();
    const batch = props.batchConnectionPreview;
    scene.batchDrafts.visible = Boolean(batch);
    if (!batch) return;
    const target = batch.targetNodeId ? props.nodeById.get(batch.targetNodeId) : undefined;
    const stroke = batch.status === "invalid" ? props.theme.accent.danger : batch.status === "partial" ? props.theme.node.activeStroke : props.theme.accent.primary;
    batch.sourceNodeIds.forEach((sourceNodeId) => {
        const source = props.nodeById.get(sourceNodeId);
        if (!source) return;
        const handle: ConnectionHandle = { nodeId: source.id, handleType: "source" };
        scene.batchDrafts.add(new Path({
            path: activeConnectionPath(source, handle, batch.mouseWorld, target, props.scriptScrollTopById[source.id] || 0, batch.targetAnchorRatio),
            stroke,
            strokeWidth: 1.4,
            strokeScaleFixed: true,
            strokeCap: "round",
            dashPattern: [8, 8],
            opacity: 0.72,
            hittable: false,
        }));
    });
}

function syncSelection(rect: Rect, selection: SelectionBox, theme: CanvasTheme) {
    rect.set({
        x: Math.min(selection.startWorldX, selection.currentWorldX),
        y: Math.min(selection.startWorldY, selection.currentWorldY),
        width: Math.abs(selection.currentWorldX - selection.startWorldX),
        height: Math.abs(selection.currentWorldY - selection.startWorldY),
        fill: theme.canvas.selectionFill,
        stroke: theme.accent.primary,
    });
}

function syncViewport(viewport: ViewportTransform, width: number, height: number, underlay: UnderlayScene, overlay: OverlayScene, props: ResolvedLeaferProps) {
    const scale = Math.max(viewport.k, 0.05);
    for (const scene of [underlay, overlay]) scene.world.set({ x: viewport.x, y: viewport.y, scaleX: scale, scaleY: scale });

    overlay.selection.strokeWidth = 1 / scale;
    overlay.selection.cornerRadius = 2 / scale;
    if (props.selectedNodeBounds) syncSelectionBounds(overlay.selectionBounds, props.selectedNodeBounds, scale);
    overlay.selectionBounds.set({ strokeWidth: 1 / scale, cornerRadius: 12 / scale });
    overlay.draft.set({ strokeWidth: 1.4 / scale, dashPattern: [8 / scale, 8 / scale] });
    overlay.guides.set({
        visible: typeof props.alignmentGuides.vertical === "number" || typeof props.alignmentGuides.horizontal === "number",
        path: guidePath(viewport, width, height, props.alignmentGuides),
        stroke: props.theme.accent.primary,
        strokeWidth: 1 / scale,
        dashPattern: [5 / scale, 5 / scale],
        opacity: 0.72,
    });
}

function isViewportPreview(container: HTMLDivElement, viewport: ViewportTransform, rasterViewport: ViewportTransform) {
    return container.dataset.canvasViewportInteracting === "true" && !sameCanvasViewport(viewport, rasterViewport);
}

function applyScenePreview(viewport: ViewportTransform, rasterViewport: ViewportTransform, ...scenes: LeaferScene[]) {
    // 将已栅格画面的屏幕坐标映射到实时视口，缩放手势期间不触碰 Leafer 场景树。
    const { ratio, x, y } = calculateCanvasPreviewTransform(viewport, rasterViewport);
    for (const scene of scenes) {
        scene.host.style.transformOrigin = "0 0";
        scene.host.style.transform = `translate3d(${x}px, ${y}px, 0) scale(${ratio})`;
        scene.host.style.willChange = "transform";
        scene.host.dataset.canvasLeaferPreview = "true";
    }
}

function hasScenePreview(...scenes: LeaferScene[]) {
    return scenes.some((scene) => scene.host.dataset.canvasLeaferPreview === "true");
}

function resetScenePreview(...scenes: LeaferScene[]) {
    for (const scene of scenes) {
        scene.host.style.transform = "";
        scene.host.style.transformOrigin = "";
        scene.host.style.willChange = "";
        delete scene.host.dataset.canvasLeaferPreview;
    }
}

function forceSceneRender(...scenes: LeaferScene[]) {
    for (const scene of scenes) scene.leafer.forceRender(undefined, true);
}

function syncSelectionBounds(rect: Rect, bounds: NonNullable<NodeBounds>, viewportScale: number) {
    const padding = 12 / Math.max(viewportScale, 0.05);
    rect.set({
        x: bounds.left - padding,
        y: bounds.top - padding,
        width: bounds.width + padding * 2,
        height: bounds.height + padding * 2,
    });
}

function guidePath(viewport: ViewportTransform, width: number, height: number, guides: { vertical?: number; horizontal?: number }) {
    const scale = Math.max(viewport.k, 0.05);
    const left = -viewport.x / scale;
    const top = -viewport.y / scale;
    const right = left + width / scale;
    const bottom = top + height / scale;
    const commands: string[] = [];
    if (typeof guides.vertical === "number") commands.push(`M ${guides.vertical} ${top} L ${guides.vertical} ${bottom}`);
    if (typeof guides.horizontal === "number") commands.push(`M ${left} ${guides.horizontal} L ${right} ${guides.horizontal}`);
    return commands.join(" ");
}

function canvasPixelRatio() {
    return Math.min(3, Math.max(1, window.devicePixelRatio || 1));
}
