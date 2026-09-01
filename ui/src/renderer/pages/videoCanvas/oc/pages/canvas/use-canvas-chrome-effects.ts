import { useEffect, useRef } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";
import { persistCanvasMediaPerformanceMode } from "@oc/lib/canvas/canvas-performance-mode";
import { persistCanvasWorkspaceMode } from "@oc/lib/canvas/canvas-project-domain";
import { useCanvasInteractionStore } from "@oc/stores/canvas/use-canvas-interaction-store";
import type { CanvasMediaPerformanceMode, CanvasWorkspaceMode } from "@oc/types/canvas";
import type { useCanvasAssistantVisibility } from "./use-canvas-assistant-visibility";

type CanvasChromeEffectsInput = {
    projectId: string;
    didInitialCenterRef: RefObject<boolean>;
    workspaceMode: CanvasWorkspaceMode;
    mediaPerformanceMode: CanvasMediaPerformanceMode;
    focusMode: boolean;
    dialogNodeId: string | null;
    searchParams: URLSearchParams;
    projectLoaded: boolean;
    openAgent: ReturnType<typeof useCanvasAssistantVisibility>["openAgent"];
    setAgentMode: ReturnType<typeof useCanvasAssistantVisibility>["setAgentMode"];
    closeAgent: () => void;
    setNodeSearchOpen: Dispatch<SetStateAction<boolean>>;
    setIsMiniMapOpen: Dispatch<SetStateAction<boolean>>;
    setFocusDockRevealed: Dispatch<SetStateAction<boolean>>;
    setNodeImageSettingsOpen: Dispatch<SetStateAction<boolean>>;
};

export function useCanvasChromeEffects(input: CanvasChromeEffectsInput) {
    const {
        projectId,
        didInitialCenterRef,
        workspaceMode,
        mediaPerformanceMode,
        focusMode,
        dialogNodeId,
        searchParams,
        projectLoaded,
        openAgent,
        closeAgent,
        setNodeSearchOpen,
        setIsMiniMapOpen,
        setFocusDockRevealed,
        setNodeImageSettingsOpen,
    } = input;

    useEffect(() => {
        persistCanvasWorkspaceMode(workspaceMode);
    }, [workspaceMode]);

    useEffect(() => {
        persistCanvasMediaPerformanceMode(mediaPerformanceMode);
    }, [mediaPerformanceMode]);

    useEffect(() => {
        didInitialCenterRef.current = false;
        useCanvasInteractionStore.getState().resetInteraction();
        return () => useCanvasInteractionStore.getState().resetInteraction();
    }, [projectId]);

    useEffect(() => {
        const openSearch = (event: KeyboardEvent) => {
            if (!(event.metaKey || event.ctrlKey) || event.key.toLocaleLowerCase() !== "k") return;
            const target = event.target;
            if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || (target instanceof HTMLElement && target.isContentEditable)) return;
            event.preventDefault();
            setNodeSearchOpen(true);
        };
        window.addEventListener("keydown", openSearch);
        return () => window.removeEventListener("keydown", openSearch);
    }, []);

    useEffect(() => {
        if (!projectLoaded || !["new", "recent", "choose"].includes(searchParams.get("mode") || "")) return;
        if (searchParams.has("agentUrl")) return;
        openAgent("online");
    }, [openAgent, projectLoaded, searchParams]);

    // 沉浸专注进入时收起智能体与小地图、重置 Dock 唤出态；仅响应「进入」瞬间，避免关闭专注内主动唤出的面板。
    const prevFocusModeRef = useRef(focusMode);
    useEffect(() => {
        const enteredFocus = focusMode && !prevFocusModeRef.current;
        prevFocusModeRef.current = focusMode;
        if (!enteredFocus) return;
        closeAgent();
        setIsMiniMapOpen(false);
        setFocusDockRevealed(false);
    }, [closeAgent, focusMode]);

    useEffect(() => {
        if (!dialogNodeId) setNodeImageSettingsOpen(false);
    }, [dialogNodeId]);
}
