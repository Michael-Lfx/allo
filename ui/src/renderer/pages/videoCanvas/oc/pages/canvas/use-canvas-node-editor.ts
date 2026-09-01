import { useCallback, useState, type Dispatch, type SetStateAction } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { App } from "antd";

import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { FRAME_COLLAPSED_HEIGHT, FRAME_COLLAPSED_WIDTH, getFrameChildIds, isFrameNode } from "@oc/lib/canvas/canvas-frame";
import { downloadCanvasNodeMedia } from "@oc/lib/canvas/canvas-node-download";
import { applyBatchPrimaryImage, applyNodeConfigPatch } from "@oc/lib/canvas/canvas-project-domain";
import { resetGenerationTaskMetadata } from "@oc/lib/canvas/canvas-project-generation";
import { CONTENT_MODERATION_ERROR_CODE, isContentModerationError } from "@oc/lib/generation-error";
import { ensureCanvasNodeAsset } from "@oc/services/project-asset-sync";
import { CanvasNodeType, type CanvasNodeData, type CanvasNodeMetadata, type Position } from "@oc/types/canvas";

type UseCanvasNodeEditorOptions = {
    canvasId: string;
    canvasTitle: string;
    domainProjectId?: string;
    nodesRef: { current: CanvasNodeData[] };
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setToolbarNodeId: Dispatch<SetStateAction<string | null>>;
    setHoveredNodeId: Dispatch<SetStateAction<string | null>>;
    onAssetSaved?: () => void;
};

export function useCanvasNodeEditor({
    canvasId,
    canvasTitle,
    domainProjectId,
    nodesRef,
    setNodes,
    setSelectedNodeIds,
    setSelectedConnectionId,
    setDialogNodeId,
    setToolbarNodeId,
    setHoveredNodeId,
    onAssetSaved,
}: UseCanvasNodeEditorOptions) {
    const { message } = App.useApp();
    const queryClient = useQueryClient();
    const [collapsingBatchIds, setCollapsingBatchIds] = useState<Set<string>>(new Set());
    const [openingBatchIds, setOpeningBatchIds] = useState<Set<string>>(new Set());

    const handleNodeResize = useCallback((nodeId: string, width: number, height: number, position?: Position, options?: { markManual?: boolean }) => {
        const markManual = options?.markManual !== false;
        setNodes((current) => {
            let changed = false;
            const next = current.map((node) => {
                if (node.id !== nodeId || node.metadata?.locked) return node;
                const nextPosition = position || node.position;
                if (node.width === width && node.height === height && node.position.x === nextPosition.x && node.position.y === nextPosition.y) return node;
                changed = true;
                // markManual 默认 true：用户拖尺寸后，图片 onLoad 比例校正不再覆盖。
                // 图片真实比例自适应传 { markManual: false }，避免一次校正就锁死。
                const resized = {
                    ...node,
                    width,
                    height,
                    position: nextPosition,
                    metadata: markManual ? { ...node.metadata, manualSize: true } : node.metadata,
                };
                if (!isFrameNode(node) || node.metadata?.frame?.collapsed) return resized;
                return { ...resized, metadata: { ...resized.metadata, frame: { collapsed: false, expandedWidth: width, expandedHeight: height } } };
            });
            return changed ? next : current;
        });
    }, [setNodes]);

    const toggleFrameCollapsed = useCallback((nodeId: string) => {
        const frame = nodesRef.current.find((node) => node.id === nodeId && isFrameNode(node));
        if (!frame) return;
        const collapsed = Boolean(frame.metadata?.frame?.collapsed);
        const childIds = getFrameChildIds(nodeId, nodesRef.current);
        setNodes((current) =>
            current.map((node) => {
                if (node.id !== nodeId) return node;
                const frameState = node.metadata?.frame;
                return collapsed
                    ? { ...node, width: frameState?.expandedWidth || NODE_DEFAULT_SIZE[CanvasNodeType.Frame].width, height: frameState?.expandedHeight || NODE_DEFAULT_SIZE[CanvasNodeType.Frame].height, metadata: { ...node.metadata, frame: { collapsed: false, expandedWidth: frameState?.expandedWidth || NODE_DEFAULT_SIZE[CanvasNodeType.Frame].width, expandedHeight: frameState?.expandedHeight || NODE_DEFAULT_SIZE[CanvasNodeType.Frame].height } } }
                    : { ...node, width: FRAME_COLLAPSED_WIDTH, height: FRAME_COLLAPSED_HEIGHT, metadata: { ...node.metadata, frame: { collapsed: true, expandedWidth: node.width, expandedHeight: node.height } } };
            }),
        );
        setSelectedNodeIds(new Set([nodeId]));
        setSelectedConnectionId(null);
        setDialogNodeId((current) => (current && childIds.has(current) ? null : current));
        setToolbarNodeId(null);
        setHoveredNodeId(null);
    }, [nodesRef, setDialogNodeId, setHoveredNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds, setToolbarNodeId]);

    const handleNodeTitleChange = useCallback((nodeId: string, title: string) => {
        setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, title } : node)));
    }, [setNodes]);

    const toggleNodeFreeResize = useCallback((nodeId: string) => {
        setNodes((current) =>
            current.map((node) => {
                if (node.id !== nodeId) return node;
                const freeResize = !node.metadata?.freeResize;
                if (freeResize || node.type !== CanvasNodeType.Image) return { ...node, metadata: { ...node.metadata, freeResize } };
                const ratio = (node.metadata?.naturalWidth || node.width) / (node.metadata?.naturalHeight || node.height || 1);
                const height = node.width / ratio;
                return { ...node, height, position: { x: node.position.x, y: node.position.y + node.height / 2 - height / 2 }, metadata: { ...node.metadata, freeResize } };
            }),
        );
    }, [setNodes]);

    const handleNodeContentChange = useCallback((nodeId: string, content: string) => {
        setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, content, richText: undefined } } : node)));
    }, [setNodes]);

    const toggleBatchExpanded = useCallback((nodeId: string) => {
        const isExpanded = Boolean(nodesRef.current.find((node) => node.id === nodeId)?.metadata?.imageBatchExpanded);
        const updateMotionState = isExpanded ? setCollapsingBatchIds : setOpeningBatchIds;
        updateMotionState((current) => new Set(current).add(nodeId));
        window.setTimeout(() => {
            updateMotionState((current) => {
                const next = new Set(current);
                next.delete(nodeId);
                return next;
            });
        }, isExpanded ? 320 : 260);
        setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, imageBatchExpanded: !node.metadata?.imageBatchExpanded } } : node)));
    }, [nodesRef, setNodes]);

    const setBatchPrimary = useCallback((child: CanvasNodeData) => {
        const rootId = child.metadata?.batchRootId;
        if (!rootId || !child.metadata?.content) return;
        setNodes((current) =>
            current.map((node) =>
                node.id === rootId
                    ? applyBatchPrimaryImage(node, child)
                    : node,
            ),
        );
    }, [setNodes]);

    const handleNodePromptChange = useCallback((nodeId: string, prompt: string) => {
        setNodes((current) => current.map((node) => {
            if (node.id !== nodeId) return node;
            const hasExistingContent = (node.type === CanvasNodeType.Text && Boolean(node.metadata?.content?.trim())) || (node.type === CanvasNodeType.Image && Boolean(node.metadata?.content));
            const previousPrompt = node.metadata?.composerContent ?? node.metadata?.prompt ?? "";
            const moderationFailure = node.metadata?.generationErrorCode === CONTENT_MODERATION_ERROR_CODE || isContentModerationError(node.metadata?.errorDetails);
            const metadata = moderationFailure && prompt !== previousPrompt
                ? resetGenerationTaskMetadata(node.metadata, node.metadata?.content ? "success" : "idle")
                : node.metadata;
            const promptTemplateMetadata = prompt !== previousPrompt && metadata?.promptTemplateOperation
                ? { promptTemplateOperation: undefined, promptTemplateVariables: undefined }
                : {};
            return { ...node, metadata: hasExistingContent ? { ...metadata, ...promptTemplateMetadata, composerContent: prompt } : { ...metadata, ...promptTemplateMetadata, prompt, composerContent: prompt } };
        }));
    }, [setNodes]);

    const handleConfigNodeChange = useCallback((nodeId: string, patch: Partial<CanvasNodeMetadata>) => {
        setNodes((current) => {
            const next = current.map((node) => (node.id === nodeId ? applyNodeConfigPatch(node, patch) : node));
            return next.some((node, index) => node !== current[index]) ? next : current;
        });
        if (!patch.assetCategory) return;
        const node = nodesRef.current.find((item) => item.id === nodeId);
        if (!node?.metadata?.content?.trim()) return;
        const updatedNode = applyNodeConfigPatch(node, patch);
        void ensureCanvasNodeAsset({ canvasId, domainProjectId, node: updatedNode, source: "canvas-manual", category: patch.assetCategory })
            .then(async (result) => {
                setNodes((current) => current.map((item) => item.id === nodeId ? { ...item, metadata: { ...item.metadata, assetId: result.assetId } } : item));
                if (domainProjectId) await queryClient.invalidateQueries({ queryKey: ["project", domainProjectId] });
                message.success("资产分类已更新");
            })
            .catch((error) => message.error(error instanceof Error ? error.message : "资产分类更新失败"));
    }, [canvasId, domainProjectId, message, nodesRef, queryClient, setNodes]);

    const downloadNodeImage = useCallback(async (node: CanvasNodeData) => {
        if (node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video && node.type !== CanvasNodeType.Audio) return;
        if (!node.metadata?.content?.trim() && !node.metadata?.storageKey?.trim()) {
            message.error("当前节点没有可下载的内容");
            return;
        }
        const hide = message.loading(node.type === CanvasNodeType.Video ? "正在保存视频…" : node.type === CanvasNodeType.Audio ? "正在保存音频…" : "正在保存图片…", 0);
        try {
            const result = await downloadCanvasNodeMedia(node, { canvasTitle });
            if (result === "saved") message.success("文件已保存");
            else message.success("已开始下载，请在浏览器下载栏查看");
        } catch (error) {
            if (error instanceof DOMException && error.name === "AbortError") return;
            message.error(error instanceof Error ? error.message : "下载失败");
        } finally {
            hide();
        }
    }, [canvasTitle, message]);

    const saveNodeAsset = useCallback(async (node: CanvasNodeData) => {
        if (node.type !== CanvasNodeType.Text && node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video && node.type !== CanvasNodeType.Audio) return message.error("当前节点类型不能保存为素材");
        if (!node.metadata?.content?.trim()) return message.error("当前节点没有可保存的内容");
        try {
            const result = await ensureCanvasNodeAsset({ canvasId, domainProjectId, node, source: "canvas-manual" });
            setNodes((current) => current.map((item) => item.id === node.id ? { ...item, metadata: { ...item.metadata, assetId: result.assetId } } : item));
            if (domainProjectId) await queryClient.invalidateQueries({ queryKey: ["project", domainProjectId] });
            message.success(result.linkedToProject ? "已加入项目资产" : "已加入我的素材");
            onAssetSaved?.();
        } catch (error) {
            message.error(error instanceof Error ? error.message : "素材保存失败");
        }
    }, [canvasId, domainProjectId, message, onAssetSaved, queryClient, setNodes]);

    const handleFontSizeChange = useCallback((nodeId: string, fontSize: number) => {
        setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, fontSize } } : node)));
    }, [setNodes]);

    return {
        collapsingBatchIds,
        downloadNodeImage,
        handleConfigNodeChange,
        handleFontSizeChange,
        handleNodeContentChange,
        handleNodePromptChange,
        handleNodeResize,
        handleNodeTitleChange,
        openingBatchIds,
        saveNodeAsset,
        setBatchPrimary,
        toggleBatchExpanded,
        toggleFrameCollapsed,
        toggleNodeFreeResize,
    };
}
