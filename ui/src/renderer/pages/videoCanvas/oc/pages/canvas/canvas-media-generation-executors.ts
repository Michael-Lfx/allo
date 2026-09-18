import { nanoid } from "nanoid";

import { NODE_DEFAULT_SIZE } from "@oc/constant/canvas";
import { canGenerateMediaInPlace, findAvailableGenerationGroupPosition, generationGridPosition, generationGridSize } from "@oc/lib/canvas/canvas-generation-layout";
import { CANVAS_VIDEO_BATCH_MAX_COUNT, getCanvasBatchCount } from "@oc/lib/canvas/canvas-generation-count";
import { canvasGenerationSeed, detectCanvasGenerationIntent, varyCanvasGenerationPrompt } from "@oc/lib/canvas/canvas-generation-enhance";
import { audioMetadata, videoMetadata } from "@oc/lib/canvas/canvas-generation-task-sync";
import { fitNodeSize, nodeSizeFromRatio, VIDEO_NODE_MAX_SIZE } from "@oc/lib/canvas/canvas-node-size";
import { nextCanvasVersionLabel } from "@oc/lib/canvas/canvas-layout";
import { buildAudioGenerationMetadata, buildVideoGenerationMetadata, generationReferenceUrls, isGenerationCanceled, runBackendCanvasGenerationTask } from "@oc/lib/canvas/canvas-project-generation";
import { CONTENT_MODERATION_ERROR_CODE, generationFailureMetadata, type GenerationFailureMetadata } from "@oc/lib/generation-error";
import { storeGeneratedAudio } from "@oc/services/api/audio";
import { storeGeneratedVideo } from "@oc/services/api/video";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import { preserveAlloVimaxOnRegenerate } from "@renderer/pages/videoCanvas/lib/alloVimaxBridge";

import type { CanvasGenerationExecution } from "./canvas-generation-executor-types";

const NODE_STATUS_LOADING = "loading" as const;
const NODE_STATUS_SUCCESS = "success" as const;
const NODE_STATUS_ERROR = "error" as const;

export async function executeVideoGeneration(execution: CanvasGenerationExecution) {
    const count = getCanvasBatchCount(execution.generationConfig.count, CANVAS_VIDEO_BATCH_MAX_COUNT);
    if (count > 1) {
        await executeVideoGenerationBatch(execution, count);
        return;
    }
    const {
        nodeId,
        sourceNode,
        effectivePrompt,
        generationConfig,
        generationContext,
        controller,
        projectId,
        canvasConnections,
        setNodes,
        setConnections,
        startGenerationRequest,
        finishGenerationRequest,
        bindGenerationTask,
        registerPendingNodeIds,
    } = execution;
    const spec = nodeSizeFromRatio(generationConfig.size, NODE_DEFAULT_SIZE[CanvasNodeType.Video].width, NODE_DEFAULT_SIZE[CanvasNodeType.Video].height) || NODE_DEFAULT_SIZE[CanvasNodeType.Video];
    const reuseSourceNode = canGenerateMediaInPlace(sourceNode, CanvasNodeType.Video);
    const isExistingVideoNode = sourceNode?.type === CanvasNodeType.Video && Boolean(sourceNode.metadata?.content) && !reuseSourceNode;
    const videoId = reuseSourceNode ? nodeId : nanoid();
    const parent = sourceNode?.position || { x: 0, y: 0 };
    const videoGenerationMetadata = buildVideoGenerationMetadata(sourceNode, generationContext);
    const videoNode: CanvasNodeData = {
        id: videoId,
        type: CanvasNodeType.Video,
        title: effectivePrompt.slice(0, 32) || "Generated Video",
        position: reuseSourceNode && sourceNode ? sourceNode.position : { x: parent.x + (sourceNode?.width || spec.width) + 96, y: parent.y },
        width: reuseSourceNode && sourceNode ? sourceNode.width : spec.width,
        height: reuseSourceNode && sourceNode ? sourceNode.height : spec.height,
        metadata: {
            ...(reuseSourceNode ? sourceNode?.metadata || {} : {}),
            prompt: effectivePrompt,
            status: NODE_STATUS_LOADING,
            errorDetails: undefined,
            generationErrorCode: undefined,
            failedPromptFingerprint: undefined,
            resourceReloadAvailable: undefined,
            model: generationConfig.model,
            size: generationConfig.size,
            seconds: generationConfig.videoSeconds,
            vquality: generationConfig.vquality,
            generateAudio: generationConfig.videoGenerateAudio,
            watermark: generationConfig.videoWatermark,
            references: generationReferenceUrls(generationContext),
            ...preserveAlloVimaxOnRegenerate(sourceNode),
            ...videoGenerationMetadata,
        },
    };
    registerPendingNodeIds([videoId]);
    setNodes((current) => {
        if (reuseSourceNode) return current.map((node) => (node.id === nodeId ? { ...node, ...videoNode } : node));
        if (!isExistingVideoNode || !sourceNode) return [...current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, status: NODE_STATUS_SUCCESS } } : node)), videoNode];
        const rootId = sourceNode.metadata?.versionOfNodeId || sourceNode.id;
        const nextLabel = nextCanvasVersionLabel(rootId, current);
        return [
            ...current.map((node) => {
                if ((node.metadata?.versionOfNodeId || node.id) !== rootId) return node;
                return { ...node, metadata: { ...node.metadata, versionOfNodeId: rootId, versionLabel: node.metadata?.versionLabel || "A", versionPrimary: false, status: node.id === nodeId ? NODE_STATUS_SUCCESS : node.metadata?.status } };
            }),
            { ...videoNode, metadata: { ...videoNode.metadata, versionOfNodeId: rootId, versionLabel: nextLabel, versionPrimary: true } },
        ];
    });
    if (!reuseSourceNode) {
        setConnections((current) => {
            if (!isExistingVideoNode) return [...current, { id: nanoid(), fromNodeId: nodeId, toNodeId: videoId }];
            return [...current, ...canvasConnections.filter((connection) => connection.toNodeId === nodeId).map((connection) => ({ ...connection, id: nanoid(), toNodeId: videoId }))];
        });
    }

    startGenerationRequest(videoId, nodeId, nodeId, controller);
    try {
        const result = await runBackendCanvasGenerationTask({ projectId, nodeId: videoId, mode: "video", prompt: effectivePrompt, config: generationConfig, referenceImages: generationContext.referenceImages, referenceVideos: generationContext.referenceVideos, referenceAudios: generationContext.referenceAudios, signal: controller.signal, metadata: { sourceNodeId: nodeId, resolvedCharacterVersions: generationContext.resolvedCharacterVersions, resolvedCharacterVoices: generationContext.resolvedCharacterVoices, promptTemplateOperation: sourceNode?.metadata?.promptTemplateOperation, promptTemplateVariables: sourceNode?.metadata?.promptTemplateVariables, ...videoGenerationMetadata }, onTaskCreated: (task) => bindGenerationTask(videoId, task) });
        if (!result.video?.dataUrl) throw new Error("后端任务没有返回视频");
        const video = await storeGeneratedVideo({
            url: result.video.dataUrl,
            storageKey: result.video.storageKey,
            mimeType: result.video.mimeType || "video/mp4",
            width: result.video.width,
            height: result.video.height,
            bytes: result.video.bytes,
            durationMs: result.video.durationMs,
        });
        const videoSize = fitNodeSize(video.width || spec.width, video.height || spec.height, VIDEO_NODE_MAX_SIZE.width, VIDEO_NODE_MAX_SIZE.height);
        setNodes((current) => current.map((node) => {
            if (node.id !== videoId) return node;
            const geometry = node.metadata?.locked ? {} : { width: videoSize.width, height: videoSize.height, position: { x: node.position.x + node.width / 2 - videoSize.width / 2, y: node.position.y + node.height / 2 - videoSize.height / 2 } };
            return { ...node, ...geometry, metadata: { ...node.metadata, ...videoMetadata(video), prompt: effectivePrompt, model: generationConfig.model, size: generationConfig.size, seconds: generationConfig.videoSeconds, vquality: generationConfig.vquality, generateAudio: generationConfig.videoGenerateAudio, watermark: generationConfig.videoWatermark, references: generationReferenceUrls(generationContext), ...videoGenerationMetadata } };
        }));
    } finally {
        finishGenerationRequest(videoId, controller);
    }
}

async function executeVideoGenerationBatch(execution: CanvasGenerationExecution, count: number) {
    const {
        nodeId,
        sourceNode,
        effectivePrompt,
        generationConfig,
        generationContext,
        controller,
        projectId,
        canvasNodes,
        setNodes,
        setConnections,
        startGenerationRequest,
        finishGenerationRequest,
        bindGenerationTask,
        registerPendingNodeIds,
        showError,
    } = execution;
    const spec = nodeSizeFromRatio(generationConfig.size, NODE_DEFAULT_SIZE[CanvasNodeType.Video].width, NODE_DEFAULT_SIZE[CanvasNodeType.Video].height) || NODE_DEFAULT_SIZE[CanvasNodeType.Video];
    const videoIds = Array.from({ length: count }, () => nanoid());
    const parent = sourceNode?.position || { x: 0, y: 0 };
    const preferred = { x: parent.x + (sourceNode?.width || spec.width) + 96, y: parent.y };
    const origin = findAvailableGenerationGroupPosition(canvasNodes, preferred, generationGridSize(spec, count));
    const videoGenerationMetadata = buildVideoGenerationMetadata(sourceNode, generationContext);
    const intent = detectCanvasGenerationIntent(effectivePrompt);
    const batchSalt = Math.floor(Math.random() * 0x7fffffff);
    const videoNodes: CanvasNodeData[] = videoIds.map((id, index) => ({
        id,
        type: CanvasNodeType.Video,
        title: `${(effectivePrompt.slice(0, 32) || "Generated Video")} · ${index + 1}`,
        position: generationGridPosition(origin, spec, index),
        width: spec.width,
        height: spec.height,
        metadata: {
            prompt: effectivePrompt,
            status: NODE_STATUS_LOADING,
            errorDetails: undefined,
            generationErrorCode: undefined,
            failedPromptFingerprint: undefined,
            resourceReloadAvailable: undefined,
            model: generationConfig.model,
            size: generationConfig.size,
            seconds: generationConfig.videoSeconds,
            vquality: generationConfig.vquality,
            generateAudio: generationConfig.videoGenerateAudio,
            watermark: generationConfig.videoWatermark,
            count,
            seed: canvasGenerationSeed(index, batchSalt),
            references: generationReferenceUrls(generationContext),
            ...preserveAlloVimaxOnRegenerate(sourceNode),
            ...videoGenerationMetadata,
        },
    }));
    registerPendingNodeIds(videoIds);
    setNodes((current) => [
        ...current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, status: NODE_STATUS_SUCCESS } } : node)),
        ...videoNodes,
    ]);
    setConnections((current) => [...current, ...videoIds.map((videoId) => ({ id: nanoid(), fromNodeId: nodeId, toNodeId: videoId }))]);

    let hasSuccess = false;
    let hasFailure = false;
    let representativeFailure: GenerationFailureMetadata | undefined;
    let representativeError: unknown;
    await Promise.all(
        videoIds.map(async (videoId, batchIndex) => {
            startGenerationRequest(videoId, nodeId, nodeId, controller);
            try {
                const seed = canvasGenerationSeed(batchIndex, batchSalt);
                const samplePrompt = varyCanvasGenerationPrompt(effectivePrompt, batchIndex, count, intent, "video");
                const result = await runBackendCanvasGenerationTask({
                    projectId,
                    nodeId: videoId,
                    mode: "video",
                    prompt: samplePrompt,
                    config: { ...generationConfig, count: "1" },
                    referenceImages: generationContext.referenceImages,
                    referenceVideos: generationContext.referenceVideos,
                    referenceAudios: generationContext.referenceAudios,
                    signal: controller.signal,
                    metadata: {
                        sourceNodeId: nodeId,
                        seed,
                        batchIndex,
                        batchCount: count,
                        resolvedCharacterVersions: generationContext.resolvedCharacterVersions,
                        resolvedCharacterVoices: generationContext.resolvedCharacterVoices,
                        promptTemplateOperation: sourceNode?.metadata?.promptTemplateOperation,
                        promptTemplateVariables: sourceNode?.metadata?.promptTemplateVariables,
                        ...videoGenerationMetadata,
                    },
                    onTaskCreated: (task) => bindGenerationTask(videoId, task),
                });
                if (!result.video?.dataUrl) throw new Error("后端任务没有返回视频");
                const video = await storeGeneratedVideo({
                    url: result.video.dataUrl,
                    storageKey: result.video.storageKey,
                    mimeType: result.video.mimeType || "video/mp4",
                    width: result.video.width,
                    height: result.video.height,
                    bytes: result.video.bytes,
                    durationMs: result.video.durationMs,
                });
                const videoSize = fitNodeSize(video.width || spec.width, video.height || spec.height, VIDEO_NODE_MAX_SIZE.width, VIDEO_NODE_MAX_SIZE.height);
                setNodes((current) => current.map((node) => {
                    if (node.id !== videoId) return node;
                    const geometry = node.metadata?.locked ? {} : { width: videoSize.width, height: videoSize.height, position: { x: node.position.x + node.width / 2 - videoSize.width / 2, y: node.position.y + node.height / 2 - videoSize.height / 2 } };
                    return { ...node, ...geometry, metadata: { ...node.metadata, ...videoMetadata(video), prompt: samplePrompt, model: generationConfig.model, size: generationConfig.size, seconds: generationConfig.videoSeconds, vquality: generationConfig.vquality, generateAudio: generationConfig.videoGenerateAudio, watermark: generationConfig.videoWatermark, references: generationReferenceUrls(generationContext), seed, ...videoGenerationMetadata } };
                }));
                hasSuccess = true;
            } catch (error) {
                if (isGenerationCanceled(error)) return;
                const failure = generationFailureMetadata(error, effectivePrompt);
                if (!representativeFailure || failure.generationErrorCode === CONTENT_MODERATION_ERROR_CODE) {
                    representativeFailure = failure;
                    representativeError = error;
                }
                hasFailure = true;
                setNodes((current) => current.map((node) => (node.id === videoId ? { ...node, metadata: { ...node.metadata, status: NODE_STATUS_ERROR, ...failure, ...(node.metadata?.taskStatus === "succeeded" ? { resourceReloadAvailable: true } : {}) } } : node)));
            } finally {
                finishGenerationRequest(videoId, controller);
            }
        }),
    );
    if (controller.signal.aborted) return;
    if (hasFailure) {
        const details = representativeFailure?.errorDetails?.trim();
        showError(hasSuccess ? (details ? `部分视频生成失败：${details}` : "部分视频生成失败") : details || "全部视频生成失败");
    }
    if (!hasSuccess && representativeError) throw representativeError;
}

export async function executeAudioGeneration({
    nodeId,
    sourceNode,
    effectivePrompt,
    generationConfig,
    generationContext,
    controller,
    projectId,
    setNodes,
    setConnections,
    startGenerationRequest,
    finishGenerationRequest,
    bindGenerationTask,
    registerPendingNodeIds,
}: CanvasGenerationExecution) {
    const spec = NODE_DEFAULT_SIZE[CanvasNodeType.Audio];
    const isEmptyAudioNode = sourceNode?.type === CanvasNodeType.Audio && !sourceNode.metadata?.content;
    const audioId = isEmptyAudioNode ? nodeId : nanoid();
    const parent = sourceNode?.position || { x: 0, y: 0 };
    const audioNode: CanvasNodeData = {
        id: audioId,
        type: CanvasNodeType.Audio,
        title: effectivePrompt.slice(0, 32) || "Generated Audio",
        position: isEmptyAudioNode ? sourceNode.position : { x: parent.x + (sourceNode?.width || spec.width) + 96, y: parent.y + ((sourceNode?.height || spec.height) - spec.height) / 2 },
        width: isEmptyAudioNode ? sourceNode.width : spec.width,
        height: isEmptyAudioNode ? sourceNode.height : spec.height,
        metadata: { prompt: effectivePrompt, status: NODE_STATUS_LOADING, ...buildAudioGenerationMetadata(generationConfig) },
    };
    registerPendingNodeIds([audioId]);
    setNodes((current) => (isEmptyAudioNode ? current.map((node) => (node.id === nodeId ? { ...node, ...audioNode } : node)) : [...current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, status: NODE_STATUS_SUCCESS } } : node)), audioNode]));
    if (!isEmptyAudioNode) setConnections((current) => [...current, { id: nanoid(), fromNodeId: nodeId, toNodeId: audioId }]);

    startGenerationRequest(audioId, nodeId, nodeId, controller);
    try {
        const result = await runBackendCanvasGenerationTask({ projectId, nodeId: audioId, mode: "audio", prompt: effectivePrompt, config: generationConfig, signal: controller.signal, metadata: { sourceNodeId: nodeId, resolvedCharacterVersions: generationContext.resolvedCharacterVersions, resolvedCharacterVoiceKey: generationContext.resolvedCharacterVoices[0]?.voiceKey }, onTaskCreated: (task) => bindGenerationTask(audioId, task) });
        if (!result.audio?.dataUrl) throw new Error("后端任务没有返回音频");
        const audio = await storeGeneratedAudio(await (await fetch(result.audio.dataUrl)).blob(), generationConfig.audioFormat);
        setNodes((current) => current.map((node) => (node.id === audioId ? { ...node, metadata: { ...node.metadata, ...audioMetadata(audio), prompt: effectivePrompt, ...buildAudioGenerationMetadata(generationConfig) } } : node)));
    } finally {
        finishGenerationRequest(audioId, controller);
    }
}
