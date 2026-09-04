import { lazy, Suspense } from "react";
import type { ReactNode } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
const CanvasEmotionWorkspace = lazy(() =>
    import("@oc/components/canvas/canvas-emotion-workspace").then((module) => ({ default: module.CanvasEmotionWorkspace })),
);
import { CanvasNodeAnglePanel } from "@oc/components/canvas/canvas-node-angle-dialog";
import { CanvasConnectionCreateMenu, CanvasNodePanelOverlay } from "@oc/components/canvas/canvas-workspace-overlays";
import { CanvasProjectSelectionToolbar } from "./canvas-project-selection-toolbar";
import { HideWhileNodeDragging, HideWhileSelectionBox } from "./canvas-project-world-layers";
import { CanvasNodeType, type CanvasNodeData, type ViewportTransform } from "@oc/types/canvas";
import type { CanvasImageEmotionPayload } from "@oc/components/canvas/canvas-node-emotion-panel";
import type { useCanvasMediaTools } from "./use-canvas-media-tools";
import type { useCanvasConnectionController } from "./use-canvas-connection-controller";
import type { useCanvasRenderModel } from "./use-canvas-render-model";
import type { useCanvasNodeOperations } from "./use-canvas-node-operations";
import type { CanvasRenderModel } from "./canvas-project-bundles";

type CanvasProjectOverlaysProps = {
    dialogNode: CanvasNodeData | null;
    viewport: ViewportTransform;
    containerRef: RefObject<HTMLDivElement | null>;
    setAngleNodeId: Dispatch<SetStateAction<string | null>>;
    generateAngleNode: ReturnType<typeof useCanvasMediaTools>["generateAngleNode"];
    setEmotionNodeId: Dispatch<SetStateAction<string | null>>;
    generateEmotionNode: ReturnType<typeof useCanvasMediaTools>["generateEmotionNode"];
    renderCanvasNodePanel: (panelNode: CanvasNodeData) => ReactNode;
    pendingConnectionCreate: ReturnType<typeof useCanvasConnectionController>["pendingConnectionCreate"];
    size: { width: number; height: number };
    canCreateDrawingFromConnection: boolean;
    createConnectedNode: ReturnType<typeof useCanvasConnectionController>["createConnectedNode"];
    cancelPendingConnectionCreate: () => void;
    selectionBoundsElementRef: RefObject<HTMLDivElement | null>;
    mergeVideoProgress: ReturnType<typeof useCanvasMediaTools>["mergeVideoProgress"];
    alignSelectedNodes: ReturnType<typeof useCanvasNodeOperations>["alignSelectedNodes"];
    arrangeSelectedNodes: ReturnType<typeof useCanvasNodeOperations>["arrangeSelectedNodes"];
    createStoryboardGroup: ReturnType<typeof useCanvasNodeOperations>["createStoryboardGroup"];
    createReferenceGroup: ReturnType<typeof useCanvasNodeOperations>["createReferenceGroup"];
    beginBatchConnectionMode: ReturnType<typeof useCanvasConnectionController>["beginBatchConnectionMode"];
    selectedNodeIds: Set<string>;
    mergeSelectedVideos: ReturnType<typeof useCanvasMediaTools>["mergeSelectedVideos"];
    renderModel: CanvasRenderModel;
};

export function CanvasProjectOverlays(props: CanvasProjectOverlaysProps) {
    const {
        dialogNode,
        viewport,
        containerRef,
        setAngleNodeId,
        generateAngleNode,
        setEmotionNodeId,
        generateEmotionNode,
        renderCanvasNodePanel,
        pendingConnectionCreate,
        size,
        canCreateDrawingFromConnection,
        createConnectedNode,
        cancelPendingConnectionCreate,
        selectionBoundsElementRef,
        mergeVideoProgress,
        alignSelectedNodes,
        arrangeSelectedNodes,
        createStoryboardGroup,
        createReferenceGroup,
        beginBatchConnectionMode,
        selectedNodeIds,
        mergeSelectedVideos,
        renderModel,
    } = props;
    const { angleNode, emotionNode, selectedNodeBounds, selectedVideoNodes } = renderModel;
    return (
        <>
                    {angleNode?.metadata?.content ? (
                        <CanvasNodePanelOverlay node={angleNode} viewport={viewport} containerRef={containerRef} panelWidth={580} panelHeight={350}>
                            <CanvasNodeAnglePanel
                                dataUrl={angleNode.metadata.content}
                                onClose={() => setAngleNodeId(null)}
                                onConfirm={(params) => {
                                    void generateAngleNode(angleNode, params);
                                }}
                            />
                        </CanvasNodePanelOverlay>
                    ) : null}

                    {emotionNode?.metadata?.content ? (
                        <Suspense fallback={null}>
                            <CanvasEmotionWorkspace
                                node={emotionNode}
                                viewport={viewport}
                                containerRef={containerRef}
                                onClose={() => setEmotionNodeId(null)}
                                onConfirm={(payload: CanvasImageEmotionPayload) => {
                                    void generateEmotionNode(emotionNode, payload);
                                }}
                            />
                        </Suspense>
                    ) : null}

                    {dialogNode && dialogNode.type !== CanvasNodeType.Script && dialogNode.type !== CanvasNodeType.Drawing ? (
                        <HideWhileSelectionBox>
                            <CanvasNodePanelOverlay node={dialogNode} viewport={viewport} containerRef={containerRef} panelWidth={520}>
                                {renderCanvasNodePanel(dialogNode)}
                            </CanvasNodePanelOverlay>
                        </HideWhileSelectionBox>
                    ) : null}

                    {pendingConnectionCreate ? (
                        <CanvasConnectionCreateMenu
                            pending={pendingConnectionCreate}
                            viewport={viewport}
                            viewportSize={size}
                            containerRef={containerRef}
                            canCreateDrawing={canCreateDrawingFromConnection}
                            onCreate={(type) => void createConnectedNode(type, pendingConnectionCreate)}
                            onClose={cancelPendingConnectionCreate}
                        />
                    ) : null}

                    {selectedNodeBounds ? (
                        <HideWhileNodeDragging>
                            <HideWhileSelectionBox>
                                <CanvasProjectSelectionToolbar
                                    anchorRef={selectionBoundsElementRef}
                                    containerRef={containerRef}
                                    count={selectedNodeBounds.count}
                                    selectedVideoCount={selectedVideoNodes.length}
                                    mergingVideos={Boolean(mergeVideoProgress)}
                                    onAlign={alignSelectedNodes}
                                    onArrange={arrangeSelectedNodes}
                                    onCreateStoryboard={createStoryboardGroup}
                                    onCreateReferenceGroup={createReferenceGroup}
                                    onBatchConnect={() => beginBatchConnectionMode(Array.from(selectedNodeIds))}
                                    onMergeVideos={() => void mergeSelectedVideos()}
                                />
                            </HideWhileSelectionBox>
                        </HideWhileNodeDragging>
                    ) : null}
        </>
    );
}
