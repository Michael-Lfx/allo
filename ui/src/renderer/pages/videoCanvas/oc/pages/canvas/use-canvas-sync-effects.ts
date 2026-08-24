import { useEffect } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
import { resolveCanvasStylePreset } from "@oc/components/canvas/canvas-style-picker-modal";
import { createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { CanvasNodeType, type CanvasNodeData, type ViewportTransform } from "@oc/types/canvas";
import type { useCanvasViewportController } from "./use-canvas-viewport-controller";

export const NODE_STATUS_SUCCESS = "success" as const;

type CanvasContainerSizeInput = {
    containerRef: RefObject<HTMLDivElement | null>;
    projectLoaded: boolean;
    viewportRef: RefObject<ViewportTransform>;
    setViewport: Dispatch<SetStateAction<ViewportTransform>>;
    setSize: Dispatch<SetStateAction<{ width: number; height: number }>>;
    didInitialCenterRef: RefObject<boolean>;
};

export function useCanvasContainerSize(input: CanvasContainerSizeInput) {
    const { containerRef, projectLoaded, viewportRef, setViewport, setSize, didInitialCenterRef } = input;

    useEffect(() => {
        if (!projectLoaded) return;
        const el = containerRef.current;
        if (!el) return;

        const updateSize = () => {
            const rect = el.getBoundingClientRect();
            setSize((current) => (current.width === rect.width && current.height === rect.height ? current : { width: rect.width, height: rect.height }));
            if (!didInitialCenterRef.current) {
                didInitialCenterRef.current = true;
                const current = viewportRef.current;
                if (current.x === 0 && current.y === 0 && current.k === 1) {
                    const centered = { x: rect.width / 2, y: rect.height / 2, k: 1 };
                    viewportRef.current = centered;
                    setViewport(centered);
                }
            }
        };

        updateSize();
        const resizeObserver = new ResizeObserver(updateSize);
        resizeObserver.observe(el);
        return () => resizeObserver.disconnect();
    }, [projectLoaded]);
}

type CanvasStylePresetSyncInput = {
    projectLoaded: boolean;
    linkedProjectQuery: { data?: { project: { stylePresetId: string | undefined } } | null | undefined };
    nodesRef: RefObject<CanvasNodeData[]>;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    getCanvasCenter: ReturnType<typeof useCanvasViewportController>["getCanvasCenter"];
};

export function useCanvasStylePresetSync(input: CanvasStylePresetSyncInput) {
    const { projectLoaded, linkedProjectQuery, nodesRef, setNodes, getCanvasCenter } = input;

    useEffect(() => {
        const preset = resolveCanvasStylePreset(linkedProjectQuery.data?.project.stylePresetId);
        if (!projectLoaded || !preset) return;
        const current = nodesRef.current.find((node) => node.type === CanvasNodeType.Text && node.metadata?.workflowKind === "styleboard");
        const nextMetadata = {
            content: preset.prompt,
            prompt: preset.prompt,
            status: NODE_STATUS_SUCCESS,
            workflowKind: "styleboard" as const,
            workflowTitle: canvasT("videoCanvas.toast.projectStyleTitle", "项目画风"),
            workflowDescription: preset.description,
            stylePresetId: preset.id,
            fontSize: 14,
            locked: true,
        };
        if (current) {
            if (current.metadata?.stylePresetId === preset.id && current.metadata?.content === preset.prompt && current.metadata?.locked) return;
            setNodes((nodes) => nodes.map((node) => (node.id === current.id ? { ...node, title: canvasT("videoCanvas.toast.projectStyleNamed", "项目画风 · {{title}}", { title: preset.title }), metadata: { ...node.metadata, ...nextMetadata } } : node)));
            return;
        }
        const node = createCanvasNode(CanvasNodeType.Text, getCanvasCenter(), nextMetadata);
        node.title = canvasT("videoCanvas.toast.projectStyleNamed", "项目画风 · {{title}}", { title: preset.title });
        node.width = 420;
        node.height = 280;
        setNodes((nodes) => [...nodes, node]);
    }, [getCanvasCenter, linkedProjectQuery.data?.project.stylePresetId, projectLoaded, setNodes]);
}
