import { useCallback } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
import { nanoid } from "nanoid";
import { getNodeSpec } from "@oc/constant/canvas";
import { createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import { getGenerationCount } from "@oc/lib/canvas/canvas-project-generation";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { App } from "antd";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";
import { NODE_STATUS_SUCCESS } from "./use-canvas-sync-effects";
import type { useEffectiveConfig } from "@oc/stores/use-config-store";
import type { useCanvasStoryboard } from "./use-canvas-storyboard";
import type { useCanvasGenerationRetry } from "./use-canvas-generation-retry";
import type { useCanvasDirector } from "./use-canvas-director";

type CanvasNodeGenerationInput = {
    effectiveConfig: ReturnType<typeof useEffectiveConfig>;
    message: ReturnType<typeof App.useApp>["message"];
    nodesRef: RefObject<CanvasNodeData[]>;
    connectionsRef: RefObject<CanvasConnection[]>;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    generateScriptRows: ReturnType<typeof useCanvasStoryboard>["generateScriptRows"];
    handleRetryNode: ReturnType<typeof useCanvasGenerationRetry>;
    openDirectorWorkbench: ReturnType<typeof useCanvasDirector>["openDirectorWorkbench"];
};

export function useCanvasNodeGenerationActions(input: CanvasNodeGenerationInput) {
    const {
        effectiveConfig,
        message,
        nodesRef,
        connectionsRef,
        setNodes,
        setConnections,
        setSelectedNodeIds,
        setSelectedConnectionId,
        setDialogNodeId,
        generateScriptRows,
        handleRetryNode,
        openDirectorWorkbench,
    } = input;

    const generateImageFromTextNode = useCallback(
        (node: CanvasNodeData) => {
            const prompt = (node.metadata?.content || node.metadata?.prompt || "").trim();
            if (!prompt) {
                message.warning(canvasT("videoCanvas.toast.emptyTextNoImage", "文本节点为空，无法生图"));
                return;
            }
            const sourceNode = nodesRef.current.find((item) => item.id === node.id);
            if (!sourceNode) return;
            const nodeSize = getNodeSpec(CanvasNodeType.Image);
            const imageNode = createCanvasNode(
                CanvasNodeType.Image,
                {
                    x: sourceNode.position.x + sourceNode.width + 96 + nodeSize.width / 2,
                    y: sourceNode.position.y + sourceNode.height / 2,
                },
                {
                    prompt: `@[node:${sourceNode.id}]`,
                    composerContent: `@[node:${sourceNode.id}]`,
                    model: effectiveConfig.imageModel || effectiveConfig.model,
                    size: effectiveConfig.size,
                    quality: effectiveConfig.quality,
                    transparentBackground: effectiveConfig.transparentBackground,
                    count: getGenerationCount(effectiveConfig.canvasImageCount || effectiveConfig.count),
                },
            );
            imageNode.title = canvasT("videoCanvas.toast.imageGeneration", "图片生成");
            const connection = { id: nanoid(), fromNodeId: sourceNode.id, toNodeId: imageNode.id };
            const nextNodes = nodesRef.current.map((item) => (item.id === sourceNode.id ? { ...item, metadata: { ...item.metadata, content: prompt, richText: undefined, prompt, status: NODE_STATUS_SUCCESS } } : item)).concat(imageNode);
            const nextConnections = [...connectionsRef.current, connection];
            nodesRef.current = nextNodes;
            connectionsRef.current = nextConnections;
            setNodes(nextNodes);
            setConnections(nextConnections);
            setSelectedNodeIds(new Set([imageNode.id]));
            setSelectedConnectionId(null);
            setDialogNodeId(imageNode.id);
        },
        [effectiveConfig, message],
    );

    const retryCanvasNode = useCallback(
        (node: CanvasNodeData) => {
            if (node.type === CanvasNodeType.Script) {
                const prompt = (node.metadata?.composerContent || node.metadata?.prompt || "").trim();
                if (!prompt) {
                    message.warning(canvasT("videoCanvas.toast.scriptMissingPlot", "分镜脚本缺少剧情内容，无法重试"));
                    return;
                }
                void generateScriptRows(node.id, prompt);
                return;
            }
            void handleRetryNode(node);
        },
        [generateScriptRows, handleRetryNode, message],
    );

    const editCanvasDirector = useCallback((node: CanvasNodeData) => openDirectorWorkbench(node.id), [openDirectorWorkbench]);
    return { generateImageFromTextNode, retryCanvasNode, editCanvasDirector };
}
