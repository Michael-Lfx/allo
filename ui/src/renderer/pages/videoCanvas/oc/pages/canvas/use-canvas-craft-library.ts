import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import { App } from "antd";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { buildGraphStarter, buildPlaybookNode, recipeApplyPatch } from "@oc/lib/canvas/craft/apply";
import { installAndLoadHubPlaybook, loadBuiltinPlaybookTemplate } from "@oc/lib/canvas/craft/hub";
import { rememberCraftRecent } from "@oc/lib/canvas/craft/recents";
import type { CraftPlaybook, CraftRecipe, LibraryTab } from "@oc/lib/canvas/craft/types";
import { fireGenerationTemplateEvent, publishGenerationTemplateFromCanvas } from "@oc/lib/canvas/generation-template/api";
import { materializeGenerationTemplate } from "@oc/lib/canvas/generation-template/apply";
import { compileGenerationTemplate, generationTemplateUserError } from "@oc/lib/canvas/generation-template/compile";
import { availableModelsFromConfig } from "@oc/lib/canvas/generation-template/agent";
import { canPublishGenerationTemplate, publishInputFromCanvasNode } from "@oc/lib/canvas/generation-template/publish";
import type { GenerationTemplateApplyPolicy, GenerationTemplateDetail } from "@oc/lib/canvas/generation-template/types";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { useConfigStore } from "@oc/stores/use-config-store";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type Position } from "@oc/types/canvas";
import type { VimaxCloudSkill } from "@renderer/pages/videoGeneration/types";

const RECIPE_NODE_TYPES = new Set<CanvasNodeType>([CanvasNodeType.Image, CanvasNodeType.Video, CanvasNodeType.Config, CanvasNodeType.Text, CanvasNodeType.Script]);

type UseCanvasCraftLibraryOptions = {
    nodesRef: { current: CanvasNodeData[] };
    connectionsRef: { current: CanvasConnection[] };
    selectedNodeIdsRef: { current: Set<string> };
    getCanvasCenter: () => Position;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setLibraryOpen: Dispatch<SetStateAction<boolean>>;
    setLibraryTab: Dispatch<SetStateAction<LibraryTab>>;
    fitCanvasSelection?: () => void;
};

export function useCanvasCraftLibrary({
    nodesRef,
    connectionsRef,
    selectedNodeIdsRef,
    getCanvasCenter,
    setNodes,
    setConnections,
    setSelectedNodeIds,
    setSelectedConnectionId,
    setLibraryOpen,
    setLibraryTab,
    fitCanvasSelection,
}: UseCanvasCraftLibraryOptions) {
    const { message, modal } = App.useApp();

    const closeLibrary = useCallback(() => setLibraryOpen(false), [setLibraryOpen]);
    const openLibrary = useCallback((tab: LibraryTab = "template") => {
        setLibraryTab(tab);
        setLibraryOpen(true);
    }, [setLibraryOpen, setLibraryTab]);

    const selectIds = useCallback((ids: string[]) => {
        const next = new Set(ids);
        selectedNodeIdsRef.current = next;
        setSelectedNodeIds(next);
        setSelectedConnectionId(null);
        fitCanvasSelection?.();
    }, [fitCanvasSelection, selectedNodeIdsRef, setSelectedConnectionId, setSelectedNodeIds]);

    const confirmTemplateDegrade = useCallback((reason?: string) => {
        const content = reason === "dropped-mixed-refs"
            ? canvasT("videoCanvas.craft.templateDegradeRefs", "该模板同时带了首尾帧和额外参考图。当前模型不能混用这两种角色，将只保留首尾帧。")
            : canvasT("videoCanvas.craft.templateDegradeModel", "首选模型不可用，将改用你当前可用的模型。");
        return new Promise<boolean>((resolve) => {
            modal.confirm({
                title: canvasT("videoCanvas.craft.templateDegradeTitle", "模板将降级套用"),
                content,
                okText: canvasT("videoCanvas.craft.templateDegradeOk", "继续套用"),
                cancelText: canvasT("videoCanvas.craft.templateDegradeCancel", "取消"),
                centered: true,
                onOk: () => resolve(true),
                onCancel: () => resolve(false),
            });
        });
    }, [modal]);

    const chooseApplyPolicy = useCallback(() => {
        return new Promise<GenerationTemplateApplyPolicy | undefined>((resolve) => {
            modal.confirm({
                title: canvasT("videoCanvas.craft.templatePolicyTitle", "当前镜头已有内容"),
                content: canvasT("videoCanvas.craft.templatePolicyBody", "替换会重写提示词并重建参考连线。也可以合并或只填槽位。可用撤销恢复。"),
                okText: canvasT("videoCanvas.craft.templatePolicyReplace", "替换套用"),
                cancelText: canvasT("videoCanvas.craft.templatePolicyOther", "其他方式"),
                centered: true,
                maskClosable: false,
                keyboard: false,
                onOk: () => resolve("replace"),
                onCancel: () => {
                    modal.confirm({
                        title: canvasT("videoCanvas.craft.templatePolicyMoreTitle", "选择套用方式"),
                        content: canvasT("videoCanvas.craft.templatePolicyMoreBody", "合并会保留已有参考连线并叠上模板资产。只填槽位不改图结构。"),
                        okText: canvasT("videoCanvas.craft.templatePolicyMerge", "合并"),
                        cancelText: canvasT("videoCanvas.craft.templatePolicySlotFill", "只填槽位"),
                        centered: true,
                        maskClosable: false,
                        keyboard: false,
                        onOk: () => resolve("merge"),
                        onCancel: () => resolve("slot-fill"),
                    });
                },
            });
        });
    }, [modal]);

    const applyTemplate = useCallback(async (detail: GenerationTemplateDetail) => {
        const selected = selectedMediaNode(nodesRef.current, selectedNodeIdsRef.current);
        try {
            let applyPolicy: GenerationTemplateApplyPolicy | undefined;
            if (selected && shotHasContent(selected, connectionsRef.current)) {
                applyPolicy = await chooseApplyPolicy();
                if (!applyPolicy) return;
            }
            const compiled = compileGenerationTemplate(detail, {
                availableModels: availableModelsFromConfig(useConfigStore.getState().config),
                selectedNodeType: selected?.type,
                currentModel: selected?.metadata?.model,
                applyPolicy,
            });
            if (compiled.degraded && !(await confirmTemplateDegrade(compiled.degradeReason))) return;
            const graph = { nodes: nodesRef.current, connections: connectionsRef.current };
            const plan = materializeGenerationTemplate(compiled, {
                ...graph,
                selectedNode: selected,
                canvasCenter: getCanvasCenter(),
            });
            nodesRef.current = plan.nodes;
            connectionsRef.current = plan.connections;
            setNodes(plan.nodes);
            setConnections(plan.connections);
            selectIds(plan.selectedNodeIds);
            fireGenerationTemplateEvent(detail.id, "apply");
            closeLibrary();
        } catch (error) {
            message.error(formatCanvasUserError(error, generationTemplateUserError(error)));
        }
    }, [chooseApplyPolicy, closeLibrary, confirmTemplateDegrade, connectionsRef, getCanvasCenter, message, nodesRef, selectIds, selectedNodeIdsRef, setConnections, setNodes]);

    const publishFromCanvas = useCallback(async () => {
        const selected = selectedMediaNode(nodesRef.current, selectedNodeIdsRef.current);
        if (!canPublishGenerationTemplate(selected)) {
            message.warning(canvasT("videoCanvas.craft.publishNeedSuccess", "先选中一个已生成成功的图片或视频节点"));
            return;
        }
        const hide = message.loading(canvasT("videoCanvas.craft.publishingTemplate", "正在发布模板"), 0);
        try {
            await publishGenerationTemplateFromCanvas(publishInputFromCanvasNode(selected, nodesRef.current));
            message.success(canvasT("videoCanvas.craft.publishSubmitted", "已提交审核"));
        } catch (error) {
            message.error(formatCanvasUserError(error, canvasT("videoCanvas.craft.publishFailed", "发布模板失败")));
        } finally {
            hide();
        }
    }, [message, nodesRef, selectedNodeIdsRef]);

    const applyRecipe = useCallback((recipe: CraftRecipe) => {
        const target = recipeTarget(nodesRef.current, selectedNodeIdsRef.current);
        if (!target) {
            message.warning(canvasT("videoCanvas.craft.selectShotFirst", "先选一个镜头节点再套手法"));
            return;
        }
        rememberCraftRecent(recipe.id);
        const patch = recipeApplyPatch(recipe, target.metadata?.composerContent ?? target.metadata?.prompt ?? "");
        setNodes((current) => {
            const next = current.map((node) => {
                if (node.id !== target.id) return node;
                return {
                    ...node,
                    metadata: {
                        ...node.metadata,
                        composerContent: patch.prompt,
                        prompt: node.type === CanvasNodeType.Text && node.metadata?.content?.trim() ? node.metadata.prompt : patch.prompt,
                        ...(patch.cameraMoveId ? { videoCameraMoveId: patch.cameraMoveId, videoCameraMovePrompt: patch.cameraMovePrompt } : {}),
                    },
                };
            });
            nodesRef.current = next;
            return next;
        });
        closeLibrary();
    }, [closeLibrary, message, nodesRef, selectedNodeIdsRef, setNodes]);

    const placePlaybook = useCallback((playbook: CraftPlaybook, template: string, coverUrl?: string) => {
        const node = buildPlaybookNode(playbook, template, getCanvasCenter(), coverUrl);
        setNodes((current) => {
            const next = [...current.map((item) => (item.metadata?.projectPlaybook ? { ...item, metadata: { ...item.metadata, projectPlaybook: false } } : item)), node];
            nodesRef.current = next;
            return next;
        });
        selectIds([node.id]);
        closeLibrary();
    }, [closeLibrary, getCanvasCenter, nodesRef, selectIds, setNodes]);

    const applyPlaybook = useCallback(async (playbook: CraftPlaybook) => {
        const hide = message.loading(canvasT("videoCanvas.craft.loadingPlaybook", "正在打开手册"), 0);
        try {
            const template = await loadBuiltinPlaybookTemplate(playbook.qualifiedId, playbook.brief);
            placePlaybook(playbook, template);
        } catch (error) {
            message.error(formatCanvasUserError(error, canvasT("videoCanvas.craft.playbookFailed", "手册打开失败")));
        } finally {
            hide();
        }
    }, [message, placePlaybook]);

    const applyGraph = useCallback((graphId: string) => {
        const built = buildGraphStarter(graphId, getCanvasCenter());
        if (!built) return;
        setNodes((current) => {
            const next = [...current, ...built.nodes];
            nodesRef.current = next;
            return next;
        });
        setConnections((current) => {
            const next = [...current, ...built.connections];
            connectionsRef.current = next;
            return next;
        });
        selectIds(built.nodes.map((node) => node.id));
        closeLibrary();
    }, [closeLibrary, connectionsRef, getCanvasCenter, nodesRef, selectIds, setConnections, setNodes]);

    const installHub = useCallback(async (skill: VimaxCloudSkill) => {
        const hide = message.loading(canvasT("videoCanvas.craft.installingHub", "正在安装社区手册"), 0);
        try {
            const packed = await installAndLoadHubPlaybook(skill.id);
            placePlaybook(packed.playbook, packed.template, packed.coverUrl);
        } catch (error) {
            message.error(formatCanvasUserError(error, canvasT("videoCanvas.craft.hubInstallFailed", "社区手册安装失败")));
        } finally {
            hide();
        }
    }, [message, placePlaybook]);

    return { openLibrary, applyRecipe, applyPlaybook, applyGraph, installHub, applyTemplate, publishFromCanvas };
}

function selectedMediaNode(nodes: CanvasNodeData[], selectedIds: Set<string>) {
    const selected = nodes.filter((node) => selectedIds.has(node.id) && (node.type === CanvasNodeType.Image || node.type === CanvasNodeType.Video));
    return selected.at(-1) || null;
}

function recipeTarget(nodes: CanvasNodeData[], selectedIds: Set<string>) {
    const selected = nodes.filter((node) => selectedIds.has(node.id) && RECIPE_NODE_TYPES.has(node.type));
    return selected.at(-1) || null;
}

function shotHasContent(node: CanvasNodeData, connections: CanvasConnection[]) {
    if ((node.metadata?.composerContent || node.metadata?.prompt || "").trim()) return true;
    return connections.some((item) => item.toNodeId === node.id);
}

