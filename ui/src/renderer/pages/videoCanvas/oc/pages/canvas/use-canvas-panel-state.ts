import { useCallback, useEffect, useState } from "react";
import { getPanelWidthBounds } from "./canvas-assistant-panel-column";
import type { Position } from "@oc/types/canvas";

export function useCanvasDialogState() {
    const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
    const [nodeSearchOpen, setNodeSearchOpen] = useState(false);
    const [nodeImageSettingsOpen, setNodeImageSettingsOpen] = useState(false);
    const [dialogNodeId, setDialogNodeId] = useState<string | null>(null);
    const [textEditorNodeId, setTextEditorNodeId] = useState<string | null>(null);
    const [characterReferenceNodeId, setCharacterReferenceNodeId] = useState<string | null>(null);
    const [drawingNodeId, setDrawingNodeId] = useState<string | null>(null);
    const [stylePickerOpen, setStylePickerOpen] = useState(false);
    const [directorTemplateRequest, setDirectorTemplateRequest] = useState<{ position?: Position } | null>(null);
    const [projectAssetOpen, setProjectAssetOpen] = useState(false);
    const [projectAssetInitialCategory, setProjectAssetInitialCategory] = useState("all");
    const [projectAssetInsertPosition, setProjectAssetInsertPosition] = useState<Position | undefined>();
    const [infoNodeId, setInfoNodeId] = useState<string | null>(null);
    const [subtitleNodeId, setSubtitleNodeId] = useState<string | null>(null);
    const [timelineNodeId, setTimelineNodeId] = useState<string | null>(null);
    const [superResolveNodeId, setSuperResolveNodeId] = useState<string | null>(null);
    const [previewNodeId, setPreviewNodeId] = useState<string | null>(null);
    const [scriptEditorNodeId, setScriptEditorNodeId] = useState<string | null>(null);
    const [scriptScrollTopById, setScriptScrollTopById] = useState<Record<string, number>>({});
    const [directorNodeId, setDirectorNodeId] = useState<string | null>(null);
    const [versionCompareRootId, setVersionCompareRootId] = useState<string | null>(null);
    const [shortcutRequestNonce, setShortcutRequestNonce] = useState(0);
    const openProjectAssets = useCallback((initialCategory = "all", position?: Position) => {
        setProjectAssetInitialCategory(initialCategory);
        setProjectAssetInsertPosition(position);
        setProjectAssetOpen(true);
    }, []);
    const closeProjectAssets = useCallback(() => {
        setProjectAssetOpen(false);
        setProjectAssetInsertPosition(undefined);
    }, []);
    return {
        clearConfirmOpen,
        setClearConfirmOpen,
        nodeSearchOpen,
        setNodeSearchOpen,
        nodeImageSettingsOpen,
        setNodeImageSettingsOpen,
        dialogNodeId,
        setDialogNodeId,
        textEditorNodeId,
        setTextEditorNodeId,
        characterReferenceNodeId,
        setCharacterReferenceNodeId,
        drawingNodeId,
        setDrawingNodeId,
        stylePickerOpen,
        setStylePickerOpen,
        directorTemplateRequest,
        setDirectorTemplateRequest,
        projectAssetOpen,
        setProjectAssetOpen,
        projectAssetInitialCategory,
        setProjectAssetInitialCategory,
        projectAssetInsertPosition,
        setProjectAssetInsertPosition,
        infoNodeId,
        setInfoNodeId,
        subtitleNodeId,
        setSubtitleNodeId,
        timelineNodeId,
        setTimelineNodeId,
        superResolveNodeId,
        setSuperResolveNodeId,
        previewNodeId,
        setPreviewNodeId,
        scriptEditorNodeId,
        setScriptEditorNodeId,
        scriptScrollTopById,
        setScriptScrollTopById,
        directorNodeId,
        setDirectorNodeId,
        versionCompareRootId,
        setVersionCompareRootId,
        shortcutRequestNonce,
        setShortcutRequestNonce,
        openProjectAssets,
        closeProjectAssets,
    };
}

export function useCanvasAssistantPanelWidth() {
    const [assistantWidth, setAssistantWidth] = useState(() => {
        const vw = typeof window !== "undefined" ? window.innerWidth : 1280;
        if (vw < 768) return 300;
        if (vw < 1024) return 360;
        if (vw < 1440) return 440;
        return 520;
    });
    useEffect(() => {
        const clamp = () => {
            const { min, max } = getPanelWidthBounds();
            setAssistantWidth((prev) => (prev < min ? min : prev > max ? max : prev));
        };
        window.addEventListener("resize", clamp);
        return () => window.removeEventListener("resize", clamp);
    }, []);
    return { assistantWidth, setAssistantWidth };
}
