import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { App } from "antd";
import { nanoid } from "nanoid";

import type { CanvasImageCropRect } from "@oc/components/canvas/canvas-node-crop-dialog";
import type { CanvasImageMaskEditPayload } from "@oc/components/canvas/canvas-node-mask-edit-dialog";
import type { CanvasImageSplitParams } from "@oc/components/canvas/canvas-node-split-dialog";
import type { CanvasImageUpscaleParams } from "@oc/components/canvas/canvas-node-upscale-dialog";
import type { CanvasImageAngleParams } from "@oc/components/canvas/canvas-node-angle-dialog";
import type { CanvasImageEmotionPayload } from "@oc/components/canvas/canvas-node-emotion-panel";
import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { cropDataUrl, splitDataUrl, upscaleDataUrl } from "@oc/lib/canvas/canvas-image-data";
import { imageMetadata, videoMetadata } from "@oc/lib/canvas/canvas-generation-task-sync";
import { buildAngleLabel, buildAnglePrompt, createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import {
    buildGenerationConfig,
    buildImageGenerationMetadata,
    nodeReferenceImage,
    isGenerationCanceled,
    runBackendCanvasGenerationTask,
} from "@oc/lib/canvas/canvas-project-generation";
import { fitNodeSize, VIDEO_NODE_MAX_SIZE } from "@oc/lib/canvas/canvas-node-size";
import { compositeEmotionImage, emotionGenerationSize } from "@oc/lib/canvas/canvas-emotion";
import { DEFAULT_PORTRAIT_TEXTURE_SETTINGS } from "@oc/lib/canvas/canvas-portrait-texture";
import { captureVideoFrames } from "@oc/lib/canvas/canvas-video-frame";
import { buildVideoFrameNodes } from "@oc/lib/canvas/canvas-video-frame-nodes";
import type { CanvasVideoFrameParams } from "@oc/components/canvas/canvas-video-frame-dialog";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { mergeVideos, type MergeVideoProgress } from "@oc/lib/canvas/canvas-video-merge";
import { generationErrorMessage, localizeGenerationErrorText } from "@oc/lib/generation-error";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { storeGeneratedVideo } from "@oc/services/api/video";
import { getMediaBlob } from "@oc/services/file-storage";
import { uploadImage } from "@oc/services/image-storage";
import type { GenerationTask } from "@oc/services/api/task-center";
import { defaultConfig, resolveModelRequestConfig, useConfigStore, useEffectiveConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type ContextMenuState } from "@oc/types/canvas";
import type { StartCanvasUploadStatus } from "./use-canvas-upload";

type UseCanvasMediaToolsOptions = {
    projectId: string;
    nodesRef: { current: CanvasNodeData[] };
    connectionsRef: { current: CanvasConnection[] };
    selectedNodeIdsRef: { current: Set<string> };
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setContextMenu: Dispatch<SetStateAction<ContextMenuState | null>>;
    setHoveredNodeId: Dispatch<SetStateAction<string | null>>;
    setToolbarNodeId: Dispatch<SetStateAction<string | null>>;
    setRunningNodeId: Dispatch<SetStateAction<string | null>>;
    startUploadStatus: StartCanvasUploadStatus;
    startGenerationRequest: (targetNodeId: string, originNodeId: string, runningId?: string, controller?: AbortController) => AbortController;
    finishGenerationRequest: (targetNodeId: string, controller: AbortController) => void;
    bindGenerationTask: (targetNodeId: string, task: GenerationTask) => void;
};

const NODE_STATUS_LOADING = "loading" as const;
const NODE_STATUS_SUCCESS = "success" as const;
const NODE_STATUS_ERROR = "error" as const;
const IMAGE_PROMPT_REVERSE_PRESET = `请根据参考图片反推一段适合用于 AI 生图的提示词。

要求：
1. 只输出提示词正文，不要解释。
2. 覆盖主体、构图、风格、光线、色彩、材质、镜头和氛围。
3. 尽量写成可直接用于生图模型的完整提示词。`;

export function useCanvasMediaTools({
    projectId,
    nodesRef,
    connectionsRef,
    selectedNodeIdsRef,
    setNodes,
    setConnections,
    setSelectedNodeIds,
    setSelectedConnectionId,
    setDialogNodeId,
    setContextMenu,
    setHoveredNodeId,
    setToolbarNodeId,
    setRunningNodeId,
    startUploadStatus,
    startGenerationRequest,
    finishGenerationRequest,
    bindGenerationTask,
}: UseCanvasMediaToolsOptions) {
    const { message } = App.useApp();
    const effectiveConfig = useEffectiveConfig();
    const isAiConfigReady = useConfigStore((state) => state.isAiConfigReady);
    const extractingVideoFrameNodeIdRef = useRef<string | null>(null);
    const mergeVideoRunningRef = useRef(false);
    const [cropNodeId, setCropNodeId] = useState<string | null>(null);
    const [annotationNodeId, setAnnotationNodeId] = useState<string | null>(null);
    const [maskEditNodeId, setMaskEditNodeId] = useState<string | null>(null);
    const [splitNodeId, setSplitNodeId] = useState<string | null>(null);
    const [upscaleNodeId, setUpscaleNodeId] = useState<string | null>(null);
    const [angleNodeId, setAngleNodeId] = useState<string | null>(null);
    const [emotionNodeId, setEmotionNodeId] = useState<string | null>(null);
    const [extractingVideoFrameNodeId, setExtractingVideoFrameNodeId] = useState<string | null>(null);
    const [frameDialogNodeId, setFrameDialogNodeId] = useState<string | null>(null);
    const [mergeVideoProgress, setMergeVideoProgress] = useState<MergeVideoProgress | null>(null);

    const createImageReversePromptNodes = useCallback((node: CanvasNodeData) => {
        if (node.type !== CanvasNodeType.Image || !node.metadata?.content) {
            message.warning("图片节点为空，无法反推提示词");
            return;
        }
        const gap = 96;
        const textSpec = NODE_DEFAULT_SIZE[CanvasNodeType.Text];
        const resultSpec = NODE_DEFAULT_SIZE[CanvasNodeType.Text];
        const centerY = node.position.y + node.height / 2;
        const textNode = {
            ...createCanvasNode(CanvasNodeType.Text, { x: node.position.x + node.width + gap + textSpec.width / 2, y: centerY }, { content: IMAGE_PROMPT_REVERSE_PRESET, prompt: IMAGE_PROMPT_REVERSE_PRESET, status: NODE_STATUS_SUCCESS, fontSize: 14 }),
            title: "反推提示词",
        };
        const resultNode = {
            ...createCanvasNode(CanvasNodeType.Text, { x: textNode.position.x + textNode.width + gap + resultSpec.width / 2, y: centerY }, {
                content: "",
                generationMode: "text",
                model: effectiveConfig.textModel || effectiveConfig.model || defaultConfig.textModel,
                count: 1,
                composerContent: `参考图片：@[node:${node.id}]\n任务说明：@[node:${textNode.id}]`,
            }),
            title: "反推提示词结果",
        };
        setNodes((current) => [...current, textNode, resultNode]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: resultNode.id }, { id: nanoid(), fromNodeId: textNode.id, toNodeId: resultNode.id }]);
        setSelectedNodeIds(new Set([resultNode.id]));
        setSelectedConnectionId(null);
        setDialogNodeId(resultNode.id);
        setContextMenu(null);
    }, [effectiveConfig.model, effectiveConfig.textModel, message, setConnections, setContextMenu, setDialogNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds]);

    const openPortraitTextureEditor = useCallback((node: CanvasNodeData) => {
        if (node.type !== CanvasNodeType.Image || !node.metadata?.content) {
            message.warning(canvasT("videoCanvas.media.emptyPortrait", "图片节点为空，无法调节人物质感"));
            return;
        }
        const portraitTextureSettings = { ...DEFAULT_PORTRAIT_TEXTURE_SETTINGS, ...node.metadata?.portraitTexture };
        const composerContent = node.metadata?.composerContent?.trim() || node.metadata?.prompt?.trim() || `@[node:${node.id}]`;
        setHoveredNodeId(null);
        setToolbarNodeId(null);
        setNodes((current) => current.map((item) => item.id === node.id ? { ...item, metadata: { ...item.metadata, prompt: composerContent, composerContent, portraitTexture: portraitTextureSettings } } : item));
        setSelectedNodeIds(new Set([node.id]));
        setSelectedConnectionId(null);
        setDialogNodeId(node.id);
    }, [message, setDialogNodeId, setHoveredNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds, setToolbarNodeId]);

    const cropImageNode = useCallback(async (node: CanvasNodeData, crop: CanvasImageCropRect) => {
        if (!node.metadata?.content) return;
        const cropped = await cropDataUrl(node.metadata.content, crop);
        const image = await uploadImage(cropped);
        const size = fitNodeSize(image.width, image.height, node.width, node.height);
        const childId = nanoid();
        const child: CanvasNodeData = { id: childId, type: CanvasNodeType.Image, title: "Cropped Image", position: { x: node.position.x + node.width + 96, y: node.position.y }, width: size.width, height: size.height, metadata: { ...imageMetadata(image), prompt: node.metadata?.prompt } };
        setNodes((current) => [...current, child]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: childId }]);
        setSelectedNodeIds(new Set([childId]));
        setDialogNodeId(childId);
        setCropNodeId(null);
    }, [setConnections, setDialogNodeId, setNodes, setSelectedNodeIds]);

    const saveAnnotatedImageNode = useCallback(async (node: CanvasNodeData, dataUrl: string) => {
        const image = await uploadImage(dataUrl);
        const size = fitNodeSize(image.width, image.height, node.width, node.height);
        const childId = nanoid();
        const child: CanvasNodeData = { id: childId, type: CanvasNodeType.Image, title: `标注 · ${node.title || "图片"}`, position: { x: node.position.x + node.width + 96, y: node.position.y }, width: size.width, height: size.height, metadata: { ...imageMetadata(image), prompt: node.metadata?.prompt } };
        setNodes((current) => [...current, child]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: childId }]);
        setSelectedNodeIds(new Set([childId]));
        setSelectedConnectionId(null);
        setDialogNodeId(null);
        setAnnotationNodeId(null);
        message.success("标注图片已保存为新节点");
    }, [message, setConnections, setDialogNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds]);

    const openVideoFrameExtractor = useCallback((node: CanvasNodeData) => {
        if (!node.metadata?.content) {
            message.warning(canvasT("videoCanvas.frames.emptyVideo", "视频节点为空，无法提取画面"));
            return;
        }
        if (extractingVideoFrameNodeIdRef.current) return;
        setHoveredNodeId(null);
        setToolbarNodeId(null);
        setFrameDialogNodeId(node.id);
    }, [message, setHoveredNodeId, setToolbarNodeId]);

    const closeFrameDialog = useCallback(() => {
        if (extractingVideoFrameNodeIdRef.current) return;
        setFrameDialogNodeId(null);
    }, []);

    const extractVideoFrames = useCallback(async (node: CanvasNodeData, params: CanvasVideoFrameParams) => {
        const content = node.metadata?.content;
        if (!content || extractingVideoFrameNodeIdRef.current || !params.timesMs.length) return;
        const progress = startUploadStatus(canvasT("videoCanvas.frames.progressTitle", "提取视频画面"), canvasT("videoCanvas.frames.progressRead", "读取视频资源"), params.timesMs.length + 2);
        extractingVideoFrameNodeIdRef.current = node.id;
        setExtractingVideoFrameNodeId(node.id);
        setFrameDialogNodeId(null);
        try {
            const storedBlob = node.metadata?.storageKey ? await getMediaBlob(node.metadata.storageKey).catch(() => null) : null;
            progress.update(canvasT("videoCanvas.frames.progressDraw", "定位并绘制所选画面"), 2);
            const captured = await captureVideoFrames(storedBlob || content, params.timesMs);
            const uploadedFrames = [];
            const uploadFailures: string[] = [];
            for (let index = 0; index < captured.frames.length; index += 1) {
                const frame = captured.frames[index];
                try {
                    progress.update(canvasT("videoCanvas.frames.progressSave", "保存画面（{{n}}/{{total}}）", { n: index + 1, total: captured.frames.length }), index + 3);
                    uploadedFrames.push({ timeMs: frame.timeMs, image: await uploadImage(frame.blob) });
                } catch (error) {
                    uploadFailures.push(error instanceof Error ? error.message : canvasT("videoCanvas.frames.uploadFailed", "画面图片上传失败"));
                }
            }
            const frameNodes = buildVideoFrameNodes(node, uploadedFrames);
            if (!frameNodes.length) throw new Error(uploadFailures[0] || canvasT("videoCanvas.frames.saveFailed", "画面图片保存失败"));
            const links = frameNodes.map((frameNode) => ({ id: nanoid(), fromNodeId: node.id, toNodeId: frameNode.id }));
            const nextNodes = [...nodesRef.current, ...frameNodes];
            const nextConnections = [...connectionsRef.current, ...links];
            const selection = new Set(frameNodes.map((frameNode) => frameNode.id));
            nodesRef.current = nextNodes;
            connectionsRef.current = nextConnections;
            selectedNodeIdsRef.current = selection;
            setNodes(nextNodes);
            setConnections(nextConnections);
            setSelectedNodeIds(selection);
            setSelectedConnectionId(null);
            const failedCount = captured.failures.length + uploadFailures.length;
            progress.done(failedCount ? canvasT("videoCanvas.frames.partialDone", "已提取 {{n}} 帧，{{failed}} 帧失败", { n: frameNodes.length, failed: failedCount }) : canvasT("videoCanvas.frames.done", "已提取 {{n}} 帧并创建图片节点", { n: frameNodes.length }));
            if (failedCount) message.warning(canvasT("videoCanvas.frames.partialWarn", "{{n}} 个时间点提取失败，其余画面已创建", { n: failedCount }));
        } catch (error) {
            const details = error instanceof Error ? error.message : canvasT("videoCanvas.frames.failed", "视频画面提取失败");
            progress.fail(details);
            message.error(details);
        } finally {
            extractingVideoFrameNodeIdRef.current = null;
            setExtractingVideoFrameNodeId(null);
        }
    }, [connectionsRef, message, nodesRef, selectedNodeIdsRef, setConnections, setNodes, setSelectedConnectionId, setSelectedNodeIds, startUploadStatus]);

    const mergeVideosByIds = useCallback(async (videoNodeIds: string[]) => {
        if (mergeVideoRunningRef.current) return;
        const requestedIds = new Set(videoNodeIds);
        const videos = nodesRef.current
            .filter((node) => requestedIds.has(node.id) && node.type === CanvasNodeType.Video && Boolean(node.metadata?.content))
            .sort((left, right) => {
                const leftShot = left.metadata?.shotIndex ?? Number.MAX_SAFE_INTEGER;
                const rightShot = right.metadata?.shotIndex ?? Number.MAX_SAFE_INTEGER;
                return leftShot - rightShot || left.position.y - right.position.y || left.position.x - right.position.x;
            });
        if (videos.length < 2) {
            message.warning("请至少选择两个已有视频");
            return;
        }
        mergeVideoRunningRef.current = true;
        setMergeVideoProgress({ phase: "loading", progress: 0 });
        try {
            const merged = await mergeVideos(videos.map((node) => ({ id: node.id, url: node.metadata?.content, storageKey: node.metadata?.storageKey })), setMergeVideoProgress);
            setMergeVideoProgress({ phase: "encoding", progress: 98 });
            const uploaded = await storeGeneratedVideo({ blob: merged });
            const size = fitNodeSize(uploaded.width || 1280, uploaded.height || 720, VIDEO_NODE_MAX_SIZE.width, VIDEO_NODE_MAX_SIZE.height);
            const left = Math.max(...videos.map((node) => node.position.x + node.width)) + 120;
            const top = Math.min(...videos.map((node) => node.position.y));
            const mergedNode = createCanvasNode(CanvasNodeType.Video, { x: left + size.width / 2, y: top + size.height / 2 }, {
                ...videoMetadata(uploaded),
                prompt: `按选中顺序合并 ${videos.length} 段视频`,
                workflowKind: "final",
                workflowTitle: "合并成片",
                videoEditOperation: "concat",
                status: NODE_STATUS_SUCCESS,
            });
            mergedNode.title = `合并成片 · ${videos.length} 段`;
            mergedNode.width = size.width;
            mergedNode.height = size.height;
            mergedNode.position = { x: left, y: top };
            const links = videos.map((node) => ({ id: nanoid(), fromNodeId: node.id, toNodeId: mergedNode.id }));
            const nextNodes = [...nodesRef.current, mergedNode];
            const nextConnections = [...connectionsRef.current, ...links];
            nodesRef.current = nextNodes;
            connectionsRef.current = nextConnections;
            setNodes(nextNodes);
            setConnections(nextConnections);
            const selection = new Set([mergedNode.id]);
            selectedNodeIdsRef.current = selection;
            setSelectedNodeIds(selection);
            setSelectedConnectionId(null);
            setDialogNodeId(null);
            setMergeVideoProgress({ phase: "encoding", progress: 100 });
            message.success(`已合并 ${videos.length} 段视频，成片节点已添加`);
        } catch (error) {
            message.error(error instanceof Error ? error.message : "视频合并失败");
        } finally {
            mergeVideoRunningRef.current = false;
            window.setTimeout(() => setMergeVideoProgress(null), 700);
        }
    }, [connectionsRef, message, nodesRef, selectedNodeIdsRef, setConnections, setDialogNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds]);

    const mergeSelectedVideos = useCallback(() => mergeVideosByIds(Array.from(selectedNodeIdsRef.current)), [mergeVideosByIds, selectedNodeIdsRef]);

    const splitImageNode = useCallback(async (node: CanvasNodeData, params: CanvasImageSplitParams) => {
        if (!node.metadata?.content) return;
        setSplitNodeId(null);
        const pieces = await splitDataUrl(node.metadata.content, params);
        const gap = 16;
        const cellWidth = node.width / params.columns;
        const cellHeight = node.height / params.rows;
        const startX = node.position.x + node.width + 96;
        const childNodes = await Promise.all(pieces.map(async (piece) => {
            const image = await uploadImage(piece.dataUrl);
            return {
                id: nanoid(),
                type: CanvasNodeType.Image,
                title: `${node.title || "图片"} ${piece.row + 1}-${piece.column + 1}`,
                position: { x: startX + piece.column * (cellWidth + gap), y: node.position.y + piece.row * (cellHeight + gap) },
                width: cellWidth,
                height: cellHeight,
                metadata: { ...imageMetadata(image), prompt: node.metadata?.prompt },
            } satisfies CanvasNodeData;
        }));
        setNodes((current) => [...current, ...childNodes]);
        setConnections((current) => [...current, ...childNodes.map((child) => ({ id: nanoid(), fromNodeId: node.id, toNodeId: child.id }))]);
        setSelectedNodeIds(new Set(childNodes.map((child) => child.id)));
        setSelectedConnectionId(null);
        setDialogNodeId(null);
        message.success(`已切分为 ${childNodes.length} 个子节点`);
    }, [message, setConnections, setDialogNodeId, setNodes, setSelectedConnectionId, setSelectedNodeIds]);

    const maskEditImageNode = useCallback(async (node: CanvasNodeData, payload: CanvasImageMaskEditPayload) => {
        if (!node.metadata?.content) return;
        const generationConfig = { ...buildGenerationConfig(effectiveConfig, node, "image"), count: "1", size: node.metadata?.size || "auto" };
        if (!isAiConfigReady(generationConfig, generationConfig.model)) {
            navigateToSettings({ continueCreation: true });
            return;
        }
        const userPrompt = payload.prompt.trim();
        const prompt = `只修改蒙版透明区域，其他区域保持不变。${userPrompt}`;
        const childId = nanoid();
        const source = nodeReferenceImage(node);
        if (!source) return;
        const generationMetadata = buildImageGenerationMetadata("edit", generationConfig, 1, [source]);
        setMaskEditNodeId(null);
        setRunningNodeId(childId);
        setNodes((current) => [...current, { id: childId, type: CanvasNodeType.Image, title: userPrompt.slice(0, 32) || "局部编辑结果", position: { x: node.position.x + node.width + 96, y: node.position.y }, width: node.width, height: node.height, metadata: { prompt, status: NODE_STATUS_LOADING, ...generationMetadata } }]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: childId }]);
        setSelectedNodeIds(new Set([childId]));
        setSelectedConnectionId(null);
        setDialogNodeId(childId);
        const controller = startGenerationRequest(childId, node.id, childId);
        try {
            const result = await runBackendCanvasGenerationTask({ projectId, nodeId: childId, mode: "image", prompt, config: generationConfig, referenceImages: [source], mask: { id: `${node.id}-mask`, name: "mask.png", type: "image/png", dataUrl: payload.maskDataUrl }, signal: controller.signal, metadata: { sourceNodeId: node.id, edit: "mask" }, onTaskCreated: (task) => bindGenerationTask(childId, task) });
            const image = result.images?.[0];
            if (!image?.dataUrl) throw new Error("后端任务没有返回图片");
            const uploaded = await uploadImage(image.dataUrl);
            const size = fitNodeSize(uploaded.width, uploaded.height, node.width, node.height);
            setNodes((current) => current.map((item) => item.id === childId ? { ...item, width: size.width, height: size.height, metadata: { ...item.metadata, ...imageMetadata(uploaded), prompt, ...generationMetadata } } : item));
        } catch (error) {
            if (isGenerationCanceled(error)) return;
            const details = generationErrorMessage(error);
            message.error(localizeGenerationErrorText(details));
            setNodes((current) => current.map((item) => item.id === childId ? { ...item, metadata: { ...item.metadata, status: NODE_STATUS_ERROR, errorDetails: details } } : item));
        } finally {
            finishGenerationRequest(childId, controller);
            setRunningNodeId(null);
        }
    }, [bindGenerationTask, effectiveConfig, finishGenerationRequest, isAiConfigReady, message, projectId, setConnections, setDialogNodeId, setNodes, setRunningNodeId, setSelectedConnectionId, setSelectedNodeIds, startGenerationRequest]);

    const upscaleImageNode = useCallback(async (node: CanvasNodeData, params: CanvasImageUpscaleParams) => {
        if (!node.metadata?.content) return;
        setUpscaleNodeId(null);
        const upscaled = await upscaleDataUrl(node.metadata.content, params);
        const image = await uploadImage(upscaled);
        const size = fitNodeSize(image.width, image.height);
        const childId = nanoid();
        const child: CanvasNodeData = { id: childId, type: CanvasNodeType.Image, title: "Upscaled Image", position: { x: node.position.x + node.width + 96, y: node.position.y }, width: size.width, height: size.height, metadata: { ...imageMetadata(image), prompt: node.metadata?.prompt } };
        setNodes((current) => [...current, child]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: childId }]);
        setSelectedNodeIds(new Set([childId]));
        setDialogNodeId(childId);
    }, [setConnections, setDialogNodeId, setNodes, setSelectedNodeIds]);

    const generateAngleNode = useCallback(async (node: CanvasNodeData, params: CanvasImageAngleParams) => {
        if (!node.metadata?.content) return;
        const generationConfig = { ...buildGenerationConfig(effectiveConfig, node, "image"), count: "1" };
        if (!isAiConfigReady(generationConfig, generationConfig.model)) {
            navigateToSettings({ continueCreation: true });
            return;
        }
        const childId = nanoid();
        const imageSpec = NODE_DEFAULT_SIZE[CanvasNodeType.Image];
        const title = buildAngleLabel(params);
        const prompt = buildAnglePrompt(params);
        const source = nodeReferenceImage(node);
        if (!source) return;
        const generationMetadata = buildImageGenerationMetadata("edit", generationConfig, 1, [source]);
        setAngleNodeId(null);
        setRunningNodeId(childId);
        setNodes((current) => [...current, { id: childId, type: CanvasNodeType.Image, title, position: { x: node.position.x + node.width + 96, y: node.position.y }, width: imageSpec.width, height: imageSpec.height, metadata: { prompt, status: NODE_STATUS_LOADING, ...generationMetadata } }]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: childId }]);
        setSelectedNodeIds(new Set([childId]));
        setDialogNodeId(childId);
        const controller = startGenerationRequest(childId, node.id, childId);
        try {
            const result = await runBackendCanvasGenerationTask({ projectId, nodeId: childId, mode: "image", prompt, config: generationConfig, referenceImages: [source], signal: controller.signal, metadata: { sourceNodeId: node.id, edit: "angle" }, onTaskCreated: (task) => bindGenerationTask(childId, task) });
            const image = result.images?.[0];
            if (!image?.dataUrl) throw new Error("后端任务没有返回图片");
            const uploaded = await uploadImage(image.dataUrl);
            const size = fitNodeSize(uploaded.width, uploaded.height, imageSpec.width, imageSpec.height);
            setNodes((current) => current.map((item) => item.id === childId ? { ...item, width: size.width, height: size.height, metadata: { ...item.metadata, ...imageMetadata(uploaded), prompt, ...generationMetadata } } : item));
        } catch (error) {
            if (isGenerationCanceled(error)) return;
            const details = generationErrorMessage(error);
            setNodes((current) => current.map((item) => item.id === childId ? { ...item, metadata: { ...item.metadata, status: NODE_STATUS_ERROR, errorDetails: details } } : item));
        } finally {
            finishGenerationRequest(childId, controller);
            setRunningNodeId(null);
        }
    }, [bindGenerationTask, effectiveConfig, finishGenerationRequest, isAiConfigReady, projectId, setConnections, setDialogNodeId, setNodes, setRunningNodeId, setSelectedNodeIds, startGenerationRequest]);

    const generateEmotionNode = useCallback(async (node: CanvasNodeData, payload: CanvasImageEmotionPayload) => {
        if (!node.metadata?.content) return;
        const baseConfig = buildGenerationConfig(effectiveConfig, node, "image");
        const providerSize = emotionGenerationSize(payload.editRegion);
        const generationConfig = { ...baseConfig, count: "1", size: providerSize, quality: !baseConfig.quality || baseConfig.quality === "auto" ? "high" : baseConfig.quality };
        if (!isAiConfigReady(generationConfig, generationConfig.model)) { navigateToSettings({ continueCreation: true }); return; }
        if (resolveModelRequestConfig(generationConfig, generationConfig.model).interfaceType !== "openai-image") {
            message.error("表情编辑需要支持蒙版的 OpenAI Images 渠道，当前渠道已拒绝整图重绘");
            return;
        }
        const source = nodeReferenceImage(node);
        if (!source) return;
        const editReference = {
            id: `${node.id}-${payload.presetId}-edit-region`,
            name: "emotion-edit-region.png",
            type: "image/png",
            dataUrl: payload.sourceDataUrl,
        };
        const characterReference = {
            id: `${node.id}-${payload.presetId}-character`,
            name: `${payload.characterName}-face.jpg`,
            type: "image/jpeg",
            dataUrl: payload.characterDataUrl,
        };
        const childId = nanoid();
        const generationMetadata = { ...buildImageGenerationMetadata("edit", generationConfig, 1, [source]), size: `${payload.imageWidth}x${payload.imageHeight}` };
        const emotionEdit = { sourceNodeId: node.id, characterName: payload.characterName, presetId: payload.presetId, intimacy: payload.intimacy, arousal: payload.arousal, label: payload.label, faceBox: payload.faceBox, editRegion: payload.editRegion, sourceWidth: payload.imageWidth, sourceHeight: payload.imageHeight, providerSize };
        setEmotionNodeId(null);
        setRunningNodeId(childId);
        setNodes((current) => [...current, { id: childId, type: CanvasNodeType.Image, title: `${payload.characterName} · ${payload.label}`, position: { x: node.position.x + node.width + 96, y: node.position.y }, width: node.width, height: node.height, metadata: { prompt: payload.prompt, status: NODE_STATUS_LOADING, ...generationMetadata, emotionEdit } }]);
        setConnections((current) => [...current, { id: nanoid(), fromNodeId: node.id, toNodeId: childId }]);
        setSelectedNodeIds(new Set([childId]));
        setSelectedConnectionId(null);
        setDialogNodeId(childId);
        const controller = startGenerationRequest(childId, node.id, childId);
        try {
            const result = await runBackendCanvasGenerationTask({ projectId, nodeId: childId, mode: "image", prompt: payload.prompt, config: generationConfig, referenceImages: [editReference, characterReference], mask: { id: `${node.id}-emotion-mask`, name: "emotion-mask.png", type: "image/png", dataUrl: payload.maskDataUrl }, signal: controller.signal, metadata: { sourceNodeId: node.id, edit: "emotion", emotion: emotionEdit }, onTaskCreated: (task) => bindGenerationTask(childId, task) });
            const image = result.images?.[0];
            if (!image?.dataUrl) throw new Error("后端任务没有返回图片");
            const composited = await compositeEmotionImage(node.metadata.content, image.dataUrl, payload.editRegion, payload.faceBox);
            const uploaded = await uploadImage(composited);
            const size = fitNodeSize(uploaded.width, uploaded.height, node.width, node.height);
            setNodes((current) => current.map((item) => item.id === childId ? { ...item, width: size.width, height: size.height, metadata: { ...item.metadata, ...imageMetadata(uploaded), prompt: payload.prompt, ...generationMetadata, emotionEdit } } : item));
        } catch (error) {
            if (isGenerationCanceled(error)) return;
            const details = generationErrorMessage(error);
            message.error(localizeGenerationErrorText(details));
            setNodes((current) => current.map((item) => item.id === childId ? { ...item, metadata: { ...item.metadata, status: NODE_STATUS_ERROR, errorDetails: details } } : item));
        } finally { finishGenerationRequest(childId, controller); setRunningNodeId(null); }
    }, [bindGenerationTask, effectiveConfig, finishGenerationRequest, isAiConfigReady, message, projectId, setConnections, setDialogNodeId, setNodes, setRunningNodeId, setSelectedConnectionId, setSelectedNodeIds, startGenerationRequest]);

    return {
        angleNodeId,
        emotionNodeId,
        annotationNodeId,
        createImageReversePromptNodes,
        openPortraitTextureEditor,
        cropImageNode,
        cropNodeId,
        closeFrameDialog,
        extractVideoFrames,
        extractingVideoFrameNodeId,
        frameDialogNodeId,
        openVideoFrameExtractor,
        generateAngleNode,
        maskEditImageNode,
        maskEditNodeId,
        mergeSelectedVideos,
        mergeVideosByIds,
        mergeVideoProgress,
        saveAnnotatedImageNode,
        setAngleNodeId,
        generateEmotionNode,
        setEmotionNodeId,
        setAnnotationNodeId,
        setCropNodeId,
        setMaskEditNodeId,
        setSplitNodeId,
        setUpscaleNodeId,
        splitImageNode,
        splitNodeId,
        upscaleImageNode,
        upscaleNodeId,
    };
}
