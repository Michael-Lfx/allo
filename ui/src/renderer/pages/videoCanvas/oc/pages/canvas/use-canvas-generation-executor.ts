import { useCallback, useRef, type Dispatch, type SetStateAction } from "react";
import { App } from "antd";

import { buildNodeGenerationContext, hydrateNodeGenerationContext } from "@oc/components/canvas/canvas-node-generation";
import type { CanvasNodeGenerationMode } from "@oc/components/canvas/canvas-node-prompt-panel";
import { canvasGenerationRequestFingerprint, canvasGenerationRequestOptions, runCanvasGenerationSubmissionOnce } from "@oc/lib/canvas/canvas-generation-submission";
import { buildGenerationConfig, isGenerationCanceled, supportsVideoReferenceAudio } from "@oc/lib/canvas/canvas-project-generation";
import { isGenerationTaskCapacityError } from "@oc/lib/canvas/canvas-generation-batch";
import { waitForInboundCanvasImages } from "@oc/lib/canvas/canvas-agent-wait";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { buildPortraitTexturePrompt } from "@oc/lib/canvas/canvas-portrait-texture";
import { collectCanvasSkills, expandSkillMentions, mergeSkillLists } from "@oc/lib/canvas/canvas-skill-mentions";
import { modelPromptLengthError } from "@oc/lib/model-capabilities";
import { generationErrorMessage, generationFailureMetadata } from "@oc/lib/generation-error";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import type { Skill } from "@oc/services/api/skills";
import type { GenerationTask } from "@oc/services/api/task-center";
import { useConfigStore, useEffectiveConfig } from "@oc/stores/use-config-store";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "@oc/types/canvas";
import { enrichPromptWithVimaxVoiceGuards } from "@renderer/pages/videoCanvas/lib/alloVimaxBridge";

import { executeImageGeneration } from "./canvas-image-generation-executor";
import { executeAudioGeneration, executeVideoGeneration } from "./canvas-media-generation-executors";
import { executeTextGeneration } from "./canvas-text-generation-executor";

type UseCanvasGenerationExecutorOptions = {
    projectId: string;
    domainProjectId?: string;
    addedSkills: Skill[];
    nodesRef: { current: CanvasNodeData[] };
    connectionsRef: { current: CanvasConnection[] };
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    setRunningNodeId: Dispatch<SetStateAction<string | null>>;
    startGenerationRequest: (targetNodeId: string, originNodeId: string, runningId?: string, controller?: AbortController) => AbortController;
    finishGenerationRequest: (targetNodeId: string, controller: AbortController) => void;
    bindGenerationTask: (targetNodeId: string, task: GenerationTask) => void;
};

const NODE_STATUS_IDLE = "idle" as const;
const NODE_STATUS_LOADING = "loading" as const;
const NODE_STATUS_ERROR = "error" as const;

export type CanvasNodeGenerationOptions = {
    controller?: AbortController;
    waitForTaskCapacity?: boolean;
    skipDuplicateConfirmation?: boolean;
};

export function useCanvasGenerationExecutor({
    projectId,
    domainProjectId,
    addedSkills,
    nodesRef,
    connectionsRef,
    setNodes,
    setConnections,
    setSelectedNodeIds,
    setSelectedConnectionId,
    setDialogNodeId,
    setRunningNodeId,
    startGenerationRequest,
    finishGenerationRequest,
    bindGenerationTask,
}: UseCanvasGenerationExecutorOptions) {
    const { message, modal } = App.useApp();
    const effectiveConfig = useEffectiveConfig();
    const isAiConfigReady = useConfigStore((state) => state.isAiConfigReady);
    const submissionLocksRef = useRef(new Map<string, Promise<unknown>>());
    const confirmDuplicateSubmission = useCallback(
        () =>
            new Promise<boolean>((resolve) => {
                modal.confirm({
                    title: canvasT("videoCanvas.generation.duplicateTitle", "再次生成相同内容？"),
                    content: canvasT("videoCanvas.generation.duplicateContent", "当前节点已使用相同提示词、模型、参数和参考素材提交过任务。再次生成会新建任务，并可能再次消耗积分。"),
                    okText: canvasT("videoCanvas.generation.duplicateOk", "仍然生成"),
                    cancelText: canvasT("videoCanvas.generation.duplicateCancel", "取消"),
                    centered: true,
                    onOk: () => resolve(true),
                    onCancel: () => resolve(false),
                });
            }),
        [modal],
    );

    return useCallback(
        (nodeId: string, mode: CanvasNodeGenerationMode, prompt: string, options?: CanvasNodeGenerationOptions) =>
            runCanvasGenerationSubmissionOnce(submissionLocksRef.current, nodeId, async () => {
            const sourceNode = nodesRef.current.find((node) => node.id === nodeId);
            if (sourceNode?.type === CanvasNodeType.Video && sourceNode.metadata?.videoEditOperation === "concat") {
                message.info("合并成片节点不直接重新生成，请重新选择源视频合并");
                return;
            }
            let generationConfig = buildGenerationConfig(effectiveConfig, sourceNode, mode);
            const hasLiveBatchChildren = sourceNode?.type === CanvasNodeType.Image && (sourceNode.metadata?.batchChildIds || []).some((childId) => nodesRef.current.some((node) => node.id === childId && node.metadata?.batchRootId === sourceNode.id));
            const hasStaleImageBatchState = mode === "image" && sourceNode?.type === CanvasNodeType.Image && !sourceNode.metadata?.content && Boolean(sourceNode.metadata?.isBatchRoot || sourceNode.metadata?.batchChildIds?.length) && !hasLiveBatchChildren;
            if (hasStaleImageBatchState) {
                setNodes((current) => current.map((node) => {
                    if (node.id !== sourceNode.id) return node;
                    const metadata = { ...node.metadata };
                    delete metadata.isBatchRoot;
                    delete metadata.batchChildIds;
                    delete metadata.primaryImageId;
                    delete metadata.imageBatchExpanded;
                    delete metadata.batchUsesReferenceImages;
                    return { ...node, metadata };
                }));
            }
            if (!isAiConfigReady(generationConfig, generationConfig.model)) {
                navigateToSettings({ continueCreation: true });
                return;
            }

            const sourceTextContent = sourceNode?.type === CanvasNodeType.Text ? sourceNode.metadata?.content?.trim() || "" : "";
            const editingTextNode = mode === "text" && Boolean(sourceTextContent);
            const generationPrompt = mode === "image" && sourceNode?.metadata?.portraitTexture
                ? buildPortraitTexturePrompt(prompt, sourceNode.metadata.portraitTexture)
                : prompt;
            const isPreparingEmptyImage = mode === "image" && sourceNode?.type === CanvasNodeType.Image && !sourceNode.metadata?.content;

            if (mode === "video") {
                await waitForInboundCanvasImages(
                    () => ({
                        projectId,
                        domainProjectId,
                        title: "",
                        nodes: nodesRef.current,
                        connections: connectionsRef.current,
                        selectedNodeIds: [],
                        viewport: { x: 0, y: 0, k: 1 },
                    }),
                    nodeId,
                );
            }

            let rawGenerationContext: Awaited<ReturnType<typeof hydrateNodeGenerationContext>>;
            const promptOnly = mode === "video";
            try {
                rawGenerationContext = await hydrateNodeGenerationContext(
                    buildNodeGenerationContext(nodeId, nodesRef.current, connectionsRef.current, editingTextNode ? `请根据要求修改以下文本。\n\n原文：\n${sourceTextContent}\n\n修改要求：\n${prompt}` : generationPrompt, promptOnly),
                    projectId,
                    domainProjectId,
                    mode,
                    mode === "video" && supportsVideoReferenceAudio(generationConfig),
                );
            } catch (error) {
                const errorDetails = generationErrorMessage(error);
                message.error(formatCanvasUserError(error));
                return;
            }

            const expandedPrompt = expandSkillMentions(rawGenerationContext.prompt, mergeSkillLists(addedSkills, collectCanvasSkills(nodesRef.current)));
            let effectivePrompt = expandedPrompt.trim();
            if (mode === "video") {
                effectivePrompt = enrichPromptWithVimaxVoiceGuards(
                    effectivePrompt,
                    sourceNode,
                    nodesRef.current,
                );
            }
            const generationContext = { ...rawGenerationContext, prompt: effectivePrompt };
            const promptLengthError = mode === "video" ? modelPromptLengthError(generationConfig, generationConfig.model, mode, effectivePrompt) : "";
            if (promptLengthError) {
                message.error(promptLengthError);
                return;
            }
            if (mode === "audio" && generationContext.characterReferences.length) {
                if (generationContext.characterReferences.length !== 1) {
                    message.error("角色配音一次只能引用一个角色卡");
                    return;
                }
                const voice = generationContext.resolvedCharacterVoices[0];
                if (!voice) {
                    message.error("角色尚未绑定可用声音，无法创建角色配音任务");
                    return;
                }
                generationConfig = { ...generationConfig, audioVoice: voice.voiceKey, audioInstructions: [voice.instructions, generationConfig.audioInstructions].filter(Boolean).join("；") };
            }
            if (!effectivePrompt && (mode === "text" || mode === "audio")) {
                return;
            }

            const requestFingerprint = canvasGenerationRequestFingerprint({
                nodeId,
                mode,
                prompt: effectivePrompt,
                model: generationConfig.model,
                options: canvasGenerationRequestOptions(generationConfig, mode),
                operation: sourceNode?.metadata?.videoEditOperation,
                audioInstructions: generationConfig.audioInstructions,
                promptTemplateOperation: sourceNode?.metadata?.promptTemplateOperation,
                promptTemplateVariables: sourceNode?.metadata?.promptTemplateVariables,
                context: generationContext,
            });
            const duplicateConfirmationRequired = !options?.skipDuplicateConfirmation && sourceNode?.metadata?.lastGenerationRequestFingerprint === requestFingerprint;
            if (duplicateConfirmationRequired && !(await confirmDuplicateSubmission())) return;

            setRunningNodeId(nodeId);
            const controller = startGenerationRequest(nodeId, nodeId, nodeId, options?.controller);
            if (controller.signal.aborted) {
                finishGenerationRequest(nodeId, controller);
                setRunningNodeId(null);
                return;
            }
            if (isPreparingEmptyImage) {
                setNodes((current) =>
                    current.map((node) =>
                        node.id === nodeId
                            ? {
                                  ...node,
                                  metadata: {
                                      ...node.metadata,
                                      prompt,
                                      status: NODE_STATUS_LOADING,
                                      taskStage: "正在准备生成任务",
                                      taskProgress: 0,
                                      taskCreatedAt: new Date().toISOString(),
                                      errorDetails: undefined,
                                      generationErrorCode: undefined,
                                      failedPromptFingerprint: undefined, resourceReloadAvailable: undefined,
                                  },
                              }
                            : node,
                    ),
                );
            }

            const markSourceStatus = sourceNode?.type !== CanvasNodeType.Image && !editingTextNode;
            const statusPrompt = sourceNode?.type === CanvasNodeType.Config ? effectivePrompt : prompt;
            if (markSourceStatus) setNodes((current) => current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, prompt: statusPrompt, status: NODE_STATUS_LOADING, errorDetails: undefined, generationErrorCode: undefined, failedPromptFingerprint: undefined, resourceReloadAvailable: undefined } } : node)));

            let pendingNodeIds: string[] = [];
            const execution = {
                projectId,
                nodeId,
                sourceNode,
                canvasNodes: nodesRef.current,
                canvasConnections: connectionsRef.current,
                prompt,
                effectivePrompt,
                generationConfig,
                generationContext,
                controller,
                editingTextNode,
                setNodes,
                setConnections,
                setSelectedNodeIds,
                setSelectedConnectionId,
                setDialogNodeId,
                startGenerationRequest,
                finishGenerationRequest,
                bindGenerationTask: (targetNodeId: string, task: GenerationTask) => {
                    bindGenerationTask(targetNodeId, task);
                    setNodes((current) => {
                        const source = current.find((node) => node.id === nodeId);
                        if (!source || source.metadata?.lastGenerationRequestFingerprint === requestFingerprint) return current;
                        return current.map((node) => (node.id === nodeId ? { ...node, metadata: { ...node.metadata, lastGenerationRequestFingerprint: requestFingerprint } } : node));
                    });
                },
                showError: (content: string) => message.error(content),
                registerPendingNodeIds: (nodeIds: string[]) => {
                    pendingNodeIds = nodeIds;
                },
            };

            try {
                if (mode === "image") await executeImageGeneration(execution);
                else if (mode === "video") await executeVideoGeneration(execution);
                else if (mode === "audio") await executeAudioGeneration(execution);
                else await executeTextGeneration(execution);
            } catch (error) {
                if (isGenerationCanceled(error)) return;
                const failure = generationFailureMetadata(error, prompt);
                if (options?.waitForTaskCapacity && isGenerationTaskCapacityError(error)) {
                    setNodes((current) => current.map((node) => {
                        if (node.id !== nodeId && !pendingNodeIds.includes(node.id)) return node;
                        const metadata = { ...(node.metadata || {}), status: NODE_STATUS_IDLE, errorDetails: undefined };
                        delete metadata.taskId;
                        delete metadata.taskStatus;
                        delete metadata.taskProgress;
                        delete metadata.taskStage;
                        delete metadata.taskCreatedAt;
                        delete metadata.taskUpdatedAt;
                        return { ...node, metadata };
                    }));
                    return;
                }
                message.error(formatCanvasUserError(failure.errorDetails));
                setNodes((current) => current.map((node) => (node.id === nodeId || pendingNodeIds.includes(node.id) ? (node.id === nodeId && !markSourceStatus ? node : { ...node, metadata: { ...node.metadata, status: NODE_STATUS_ERROR, ...failure, ...((mode === "image" || mode === "video" || mode === "audio") && node.metadata?.taskStatus === "succeeded" ? { resourceReloadAvailable: true } : {}) } }) : node)));
            } finally {
                finishGenerationRequest(nodeId, controller);
                setRunningNodeId(null);
            }
        }),
        [addedSkills, bindGenerationTask, confirmDuplicateSubmission, domainProjectId, effectiveConfig, finishGenerationRequest, isAiConfigReady, message, nodesRef, connectionsRef, projectId, setConnections, setDialogNodeId, setNodes, setRunningNodeId, setSelectedConnectionId, setSelectedNodeIds, startGenerationRequest],
    );
}
