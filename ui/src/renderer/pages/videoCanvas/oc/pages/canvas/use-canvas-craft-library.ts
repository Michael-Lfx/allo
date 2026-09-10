import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import { App } from "antd";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { buildGraphStarter, buildPlaybookNode, recipeApplyPatch } from "@oc/lib/canvas/craft/apply";
import { installAndLoadHubPlaybook, loadBuiltinPlaybookTemplate } from "@oc/lib/canvas/craft/hub";
import { rememberCraftRecent } from "@oc/lib/canvas/craft/recents";
import type { CraftPlaybook, CraftRecipe, LibraryTab } from "@oc/lib/canvas/craft/types";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
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
    const { message } = App.useApp();

    const closeLibrary = useCallback(() => setLibraryOpen(false), [setLibraryOpen]);
    const openLibrary = useCallback((tab: LibraryTab = "recipe") => {
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

    return { openLibrary, applyRecipe, applyPlaybook, applyGraph, installHub };
}

function recipeTarget(nodes: CanvasNodeData[], selectedIds: Set<string>) {
    const selected = nodes.filter((node) => selectedIds.has(node.id) && RECIPE_NODE_TYPES.has(node.type));
    return selected.at(-1) || null;
}
