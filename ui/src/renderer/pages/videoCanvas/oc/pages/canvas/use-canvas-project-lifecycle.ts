import { useCallback, useEffect, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { App } from "antd";
import { useNavigate } from "react-router-dom";

import type { CanvasBackgroundMode } from "@oc/lib/canvas-theme";
import { canvasAppearanceBaseTheme, normalizeCanvasAppearance, type CanvasAppearance } from "@oc/lib/canvas/canvas-appearance";
import { removeCanvasDrawing } from "@oc/lib/canvas/canvas-drawing-storage";
import { hydrateAssistantImages, hydrateCanvasImages, resetInterruptedGeneration } from "@oc/lib/canvas/canvas-project-generation";
import { normalizeCanvasNodeTimestamps } from "@oc/lib/canvas/canvas-node-timestamps";
import { listAddedSkills, type Skill } from "@oc/services/api/skills";
import { createCanvasProjectWithRemoteSync, saveRemoteUserDataNow } from "@oc/services/user-data-sync";
import { flushCanvasStorePersistence, useCanvasStore } from "@oc/stores/canvas/use-canvas-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { CanvasAssistantSession, CanvasConnection, CanvasNodeData, CanvasNodeMetadata, ViewportTransform } from "@oc/types/canvas";
import { createCanvasPersistPause } from "../../../lib/canvasProjectAutosave";
import type { CanvasHistorySnapshot } from "./use-canvas-history";

type UseCanvasProjectLifecycleOptions = {
    projectId: string;
    projectLoaded: boolean;
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    chatSessions: CanvasAssistantSession[];
    activeChatId: string | null;
    backgroundMode: CanvasBackgroundMode;
    canvasAppearance: CanvasAppearance;
    showImageInfo: boolean;
    viewport: ViewportTransform;
    nodesRef: MutableRefObject<CanvasNodeData[]>;
    connectionsRef: MutableRefObject<CanvasConnection[]>;
    viewportRef: MutableRefObject<ViewportTransform>;
    historyPausedRef: MutableRefObject<boolean>;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setChatSessions: Dispatch<SetStateAction<CanvasAssistantSession[]>>;
    setActiveChatId: Dispatch<SetStateAction<string | null>>;
    setBackgroundMode: Dispatch<SetStateAction<CanvasBackgroundMode>>;
    setCanvasAppearance: Dispatch<SetStateAction<CanvasAppearance>>;
    setShowImageInfo: Dispatch<SetStateAction<boolean>>;
    setViewport: Dispatch<SetStateAction<ViewportTransform>>;
    setProjectLoaded: Dispatch<SetStateAction<boolean>>;
    resetHistory: (snapshot: CanvasHistorySnapshot) => void;
    cleanupAssetImages: (options?: unknown) => void;
    cleanupCanvasFiles: (extra?: unknown) => void;
};

export function useCanvasProjectLifecycle({
    projectId,
    projectLoaded,
    nodes,
    connections,
    chatSessions,
    activeChatId,
    backgroundMode,
    canvasAppearance,
    showImageInfo,
    viewport,
    nodesRef,
    connectionsRef,
    viewportRef,
    historyPausedRef,
    setNodes,
    setConnections,
    setChatSessions,
    setActiveChatId,
    setBackgroundMode,
    setCanvasAppearance,
    setShowImageInfo,
    setViewport,
    setProjectLoaded,
    resetHistory,
    cleanupAssetImages,
    cleanupCanvasFiles,
}: UseCanvasProjectLifecycleOptions) {
    const { message } = App.useApp();
    const navigate = useNavigate();
    const hydrated = useCanvasStore((state) => state.hydrated);
    const openProject = useCanvasStore((state) => state.openProject);
    const updateProject = useCanvasStore((state) => state.updateProject);
    const renameProject = useCanvasStore((state) => state.renameProject);
    const deleteProjects = useCanvasStore((state) => state.deleteProjects);
    const currentProject = useCanvasStore((state) => state.projects.find((project) => project.id === projectId));
    const [addedSkills, setAddedSkills] = useState<Skill[]>([]);
    const viewportSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    // 打开工程后的首次媒体 hydrate 完成前，禁止把 store 变化持久化：
    // hydrate 的 setNodes/setChatSessions 不是用户编辑，落盘会触发一串
    // 无意义的 doc PUT（历史上还会把一次性 blob: URL 写进服务端文档）。
    const persistPausedRef = useRef(createCanvasPersistPause());

    useEffect(() => {
        if (!hydrated) return;
        let cancelled = false;
        setProjectLoaded(false);
        persistPausedRef.current.pause();
        const project = openProject(projectId);
        if (!project) {
            navigate("/video-generation?mode=creation", { replace: true });
            return;
        }

        const applyRestoredProject = (restoredNodes: CanvasNodeData[], restoredSessions: CanvasAssistantSession[]) => {
            if (cancelled) return;
            const snapshot: CanvasHistorySnapshot = {
                nodes: restoredNodes,
                connections: project.connections,
                chatSessions: restoredSessions,
                activeChatId: project.activeChatId || null,
                backgroundMode: project.backgroundMode || "lines",
                showImageInfo: project.showImageInfo || false,
            };
            nodesRef.current = snapshot.nodes;
            connectionsRef.current = snapshot.connections;
            viewportRef.current = project.viewport;
            setNodes(snapshot.nodes);
            setConnections(snapshot.connections);
            setChatSessions(snapshot.chatSessions);
            setActiveChatId(snapshot.activeChatId);
            setBackgroundMode(snapshot.backgroundMode);
            const restoredAppearance = normalizeCanvasAppearance(project.appearance, canvasAppearanceBaseTheme(project.appearance, "dark"));
            setCanvasAppearance(restoredAppearance);
            const restoredTheme = canvasAppearanceBaseTheme(restoredAppearance, useThemeStore.getState().theme);
            if (restoredTheme !== useThemeStore.getState().theme) useThemeStore.getState().setTheme(restoredTheme);
            setShowImageInfo(snapshot.showImageInfo);
            setViewport(project.viewport);
            resetHistory(snapshot);
            setProjectLoaded(true);
        };

        const restore = async () => {
            const initialNodes = normalizeCanvasNodeTimestamps(resetInterruptedGeneration(project.nodes), {
                createdAt: project.createdAt,
                updatedAt: project.updatedAt,
            });
            const initialSessions = project.chatSessions || [];

            // 先恢复可交互的节点和布局，媒体缓存/资源校验放到后台，避免首屏被远程资源拖住。
            applyRestoredProject(initialNodes, initialSessions);
            const [nodesResult, sessionsResult] = await Promise.allSettled([hydrateCanvasImages(initialNodes), hydrateAssistantImages(initialSessions)]);
            if (cancelled) return;
            if (nodesResult.status === "fulfilled") setNodes((current) => mergeHydratedNodeMedia(current, initialNodes, nodesResult.value));
            if (sessionsResult.status === "fulfilled") setChatSessions((current) => mergeHydratedSessions(current, sessionsResult.value));
            if (nodesResult.status === "rejected" || sessionsResult.status === "rejected") message.warning("部分本地媒体恢复失败，已使用项目记录继续打开");
            // 合并已调度完成，恢复持久化；随后的首次 updateProject 携带的就是
            // 干净的合并结果，而不是 hydrate 过程的中间态。
            persistPausedRef.current.resume();
        };
        void restore();
        return () => {
            cancelled = true;
        };
    }, [hydrated, message, navigate, openProject, projectId, resetHistory, setActiveChatId, setBackgroundMode, setChatSessions, setConnections, setNodes, setShowImageInfo, setViewport]);

    useEffect(() => {
        if (!projectLoaded) return;
        let cancelled = false;
        listAddedSkills()
            .then(({ skills }) => {
                if (!cancelled) setAddedSkills(skills);
            })
            .catch(() => {
                if (!cancelled) setAddedSkills([]);
            });
        return () => {
            cancelled = true;
        };
    }, [projectLoaded]);

    useEffect(() => {
        if (!projectLoaded || historyPausedRef.current || persistPausedRef.current.paused) return;
        updateProject(projectId, { nodes, connections, chatSessions, activeChatId, appearance: canvasAppearance, backgroundMode, showImageInfo });
    }, [activeChatId, backgroundMode, canvasAppearance, chatSessions, connections, historyPausedRef, nodes, projectId, projectLoaded, showImageInfo, updateProject]);

    useEffect(() => {
        if (!projectLoaded || persistPausedRef.current.paused) return;
        if (viewportSaveTimerRef.current) clearTimeout(viewportSaveTimerRef.current);
        viewportSaveTimerRef.current = setTimeout(() => {
            updateProject(projectId, { viewport: viewportRef.current });
            viewportSaveTimerRef.current = null;
        }, 500);
        return () => {
            if (viewportSaveTimerRef.current) clearTimeout(viewportSaveTimerRef.current);
        };
    }, [projectId, projectLoaded, updateProject, viewport, viewportRef]);

    useEffect(() => () => {
        if (!projectLoaded) return;
        if (viewportSaveTimerRef.current) clearTimeout(viewportSaveTimerRef.current);
        updateProject(projectId, { viewport: viewportRef.current });
    }, [projectId, projectLoaded, updateProject, viewportRef]);

    const createAndOpenProject = useCallback(() => {
        void createCanvasProjectWithRemoteSync(`自由画布 ${useCanvasStore.getState().projects.length + 1}`).then(({ id, syncError }) => {
            if (syncError) message.warning(syncError instanceof Error ? `画布已在本地创建，云端同步失败：${syncError.message}` : "画布已在本地创建，云端同步失败");
            navigate(`/canvas/${id}`);
        });
    }, [message, navigate]);

    const deleteCurrentProject = useCallback(() => {
        const drawingIds = nodesRef.current.flatMap((node) => node.type === "drawing" && node.metadata?.drawingId ? [node.metadata.drawingId] : []);
        if (drawingIds.length) {
            void Promise.all(drawingIds.map((drawingId) => removeCanvasDrawing(projectId, drawingId)))
                .catch(() => message.warning("项目已删除，但部分本地绘图缓存清理失败"));
        }
        deleteProjects([projectId]);
        cleanupAssetImages();
        navigate("/video-generation?mode=creation");
    }, [cleanupAssetImages, deleteProjects, message, navigate, nodesRef, projectId]);

    const renameCurrentProject = useCallback((title: string) => {
        renameProject(projectId, title);
    }, [projectId, renameProject]);

    const saveCanvasProject = useCallback(async () => {
        try {
            updateProject(projectId, {
                nodes: nodesRef.current,
                connections: connectionsRef.current,
                chatSessions,
                activeChatId,
                appearance: canvasAppearance,
                backgroundMode,
                showImageInfo,
                viewport: viewportRef.current,
                directorScenes: currentProject?.directorScenes || [],
            });
            await flushCanvasStorePersistence();
        } catch {
            message.error("画布保存失败，请稍后重试");
            return;
        }
        try {
            await saveRemoteUserDataNow();
            message.success("画布布局和位置已保存");
        } catch (error) {
            const detail = error instanceof Error ? error.message : "未知错误";
            message.warning(`本地画布布局已保存，云端同步失败：${detail}`);
        }
    }, [activeChatId, backgroundMode, canvasAppearance, chatSessions, connectionsRef, currentProject?.directorScenes, message, nodesRef, projectId, showImageInfo, updateProject, viewportRef]);

    const clearCanvasFiles = useCallback(() => {
        cleanupCanvasFiles({ projectId, nodes: [], chatSessions: [] });
    }, [cleanupCanvasFiles, projectId]);

    return {
        addedSkills,
        clearCanvasFiles,
        createAndOpenProject,
        currentProject,
        deleteCurrentProject,
        renameCurrentProject,
        saveCanvasProject,
        updateProject,
    };
}

const hydratedMediaMetadataKeys = ["content", "storageKey", "mediaId", "naturalWidth", "naturalHeight", "bytes", "mimeType", "durationMs"] as const satisfies readonly (keyof CanvasNodeMetadata)[];

function mergeHydratedNodeMedia(currentNodes: CanvasNodeData[], initialNodes: CanvasNodeData[], hydratedNodes: CanvasNodeData[]) {
    const initialById = new Map(initialNodes.map((node) => [node.id, node]));
    const hydratedById = new Map(hydratedNodes.map((node) => [node.id, node]));
    return currentNodes.map((node) => {
        const initial = initialById.get(node.id);
        const hydrated = hydratedById.get(node.id);
        if (!initial || !hydrated || node.metadata?.content !== initial.metadata?.content) return node;
        const metadata = { ...node.metadata } as CanvasNodeMetadata;
        hydratedMediaMetadataKeys.forEach((key) => {
            const value = hydrated.metadata?.[key];
            if (value !== undefined) (metadata as Record<string, unknown>)[key] = value;
        });
        return { ...node, metadata };
    });
}

function mergeHydratedSessions(currentSessions: CanvasAssistantSession[], hydratedSessions: CanvasAssistantSession[]) {
    const hydratedById = new Map(hydratedSessions.map((session) => [session.id, session]));
    return currentSessions.map((session) => {
        const hydrated = hydratedById.get(session.id);
        if (!hydrated) return session;
        const hydratedMessages = new Map(hydrated.messages.map((message) => [message.id, message]));
        return {
            ...session,
            messages: session.messages.map((message) => {
                const hydratedMessage = hydratedMessages.get(message.id);
                if (!hydratedMessage || !message.references?.length) return message;
                const hydratedReferences = new Map((hydratedMessage.references || []).map((reference) => [reference.id, reference]));
                return {
                    ...message,
                    references: message.references.map((reference) => {
                        const hydratedReference = hydratedReferences.get(reference.id);
                        return hydratedReference ? { ...reference, dataUrl: hydratedReference.dataUrl, storageKey: hydratedReference.storageKey } : reference;
                    }),
                };
            }),
        };
    });
}
