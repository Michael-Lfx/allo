import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { Switch } from "antd";
import { Info } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CanvasCreateMenu, type CanvasCreateCommand } from "@oc/components/canvas/canvas-create-menu";
import { CanvasOverlay } from "@oc/components/canvas/canvas-overlay";
import { FloatingDock } from "@oc/components/ui/aceternity/floating-dock";
import { CanvasAppearanceControls } from "@oc/components/canvas/canvas-appearance-controls";
import { ToolbarSettingsModal } from "@oc/components/canvas/toolbars/toolbar-settings-modal";
import { aceternityMotion } from "@oc/lib/aceternity-motion";
import { canvasDockStyle } from "@oc/lib/canvas/canvas-aceternity-style";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes, type CanvasBackgroundMode, type CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasAppearance } from "@oc/lib/canvas/canvas-appearance";
import { defaultToolbarPrefs, readToolbarPrefs, resolveAddNodeMenuCommands, resolveToolbarEntries, type ResolvedAddNodeMenuCommand, type ToolContext, type ToolbarHandlers, type ToolbarPrefs } from "@oc/lib/canvas/tool-registry";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { CanvasNodeType, CanvasToolMode, CanvasWorkspaceMode } from "@oc/types/canvas";

export function CanvasToolbar({
    selectedCount,
    workspaceMode,
    canvasTool,
    onToolChange,
    isProjectLinked,
    canUndo,
    canRedo,
    backgroundMode,
    appearance,
    showImageInfo,
    onAddImage,
    onAddVideo,
    onAddAudio,
    onAddText,
    onChooseStyle,
    onAddScript,
    onAddFrame,
    onAddFolder,
    onAddDrawing,
    onOpenDirector,
    onAddExtensionNode,
    onUndo,
    onRedo,
    onUpload,
    onDelete,
    onClear,
    onDeselect,
    onBackgroundModeChange,
    onAppearanceChange,
    onSaveAppearanceDefault,
    onShowImageInfoChange,
    onOpenMyAssets,
    onOpenProjectCharacters,
}: {
    selectedCount: number;
    workspaceMode: CanvasWorkspaceMode;
    canvasTool: CanvasToolMode;
    onToolChange: (tool: CanvasToolMode) => void;
    isProjectLinked: boolean;
    canUndo: boolean;
    canRedo: boolean;
    backgroundMode: CanvasBackgroundMode;
    appearance: CanvasAppearance;
    showImageInfo: boolean;
    onAddImage: () => void;
    onAddVideo: () => void;
    onAddAudio: () => void;
    onAddText: () => void;
    onChooseStyle: () => void;
    onAddScript: () => void;
    onAddFrame: () => void;
    onAddFolder: () => void;
    onAddDrawing: () => void;
    onOpenDirector: () => void;
    onAddExtensionNode: (type: CanvasNodeType) => void;
    onUndo: () => void;
    onRedo: () => void;
    onUpload: () => void;
    onDelete: () => void;
    onClear: () => void;
    onDeselect: () => void;
    onBackgroundModeChange: (mode: CanvasBackgroundMode) => void;
    onAppearanceChange: (appearance: CanvasAppearance) => void;
    onSaveAppearanceDefault: (appearance: CanvasAppearance) => void;
    onShowImageInfoChange: (show: boolean) => void;
    onOpenMyAssets: () => void;
    onOpenProjectCharacters: () => void;
}) {
    useTranslation();
    const rootRef = useRef<HTMLDivElement>(null);
    const dockRef = useRef<HTMLDivElement>(null);
    const colorTheme = useThemeStore((state) => state.theme);
    const theme = canvasThemes[colorTheme];
    const [addOpen, setAddOpen] = useState(false);
    const [appearanceOpen, setAppearanceOpen] = useState(false);
    const [settingsOpen, setSettingsOpen] = useState(false);
    const [panelX, setPanelX] = useState(0);
    const [prefs, setPrefs] = useState<ToolbarPrefs | null>(() => readToolbarPrefs("main"));

    // 设置面板关闭后重新读取偏好（用户可能调整了排序/显隐）
    useEffect(() => {
        if (!settingsOpen) setPrefs(readToolbarPrefs("main"));
    }, [settingsOpen]);

    const placePanel = (event: ReactMouseEvent<HTMLElement>) => setPanelX(getPanelX(dockRef.current, event.currentTarget));
    const runAddAction = (action: () => void) => {
        action();
        setAddOpen(false);
    };

    // 点击外部关闭浮层面板
    useEffect(() => {
        if (!addOpen && !appearanceOpen) return;
        const closeFloatingPanels = (event: PointerEvent) => {
            const target = event.target instanceof Node ? event.target : null;
            if (target && rootRef.current?.contains(target)) return;
            if (target instanceof Element && target.closest(".ant-color-picker,.ant-color-picker-dropdown,.ant-popover,.ant-select-dropdown")) return;
            setAddOpen(false);
            setAppearanceOpen(false);
        };
        document.addEventListener("pointerdown", closeFloatingPanels, true);
        return () => document.removeEventListener("pointerdown", closeFloatingPanels, true);
    }, [addOpen, appearanceOpen]);

    // 构建 handlers（主工具栏只需要部分回调，其余用 no-op 占位满足类型）
    const handlers: ToolbarHandlers = {
        onToolChange,
        onDeselect,
        onUndo,
        onRedo,
        onClear,
        onAddText,
        onAddImage,
        onAddVideo,
        onAddAudio,
        onAddScript,
        onAddFrame,
        onAddFolder,
        onAddDrawing,
        onChooseStyle,
        onOpenDirector,
        onAddExtensionNode,
        onUpload,
        onOpenMyAssets,
        onOpenProjectCharacters,
        onBackgroundModeChange,
        onShowImageInfoChange,
        onToggleAddPanel: (event: ReactMouseEvent<HTMLElement>) => { placePanel(event); setAppearanceOpen(false); setSettingsOpen(false); setAddOpen((value) => !value); },
        onToggleAppearancePanel: (event: ReactMouseEvent<HTMLElement>) => { placePanel(event); setAddOpen(false); setSettingsOpen(false); setAppearanceOpen((value) => !value); },
        onToggleSettingsPanel: () => { setAddOpen(false); setAppearanceOpen(false); setSettingsOpen((value) => !value); },
        onDeleteSelected: onDelete,
        // 以下为多选/节点悬停工具栏回调，主工具栏不使用，用 no-op 占位
        onAlign: () => {}, onArrange: () => {}, onCreateStoryboard: () => {}, onCreateReferenceGroup: () => {}, onBatchConnect: () => {}, onMergeVideos: () => {},
        onNodeInfo: () => {}, onNodeDelete: () => {}, onNodeRetry: () => {}, onNodeEditText: () => {}, onNodeDecreaseFont: () => {}, onNodeIncreaseFont: () => {},
        onNodeToggleDialog: () => {}, onNodeAnnotate: () => {}, onNodeGenerateImage: () => {}, onNodeUpload: () => {}, onNodeDownload: () => {}, onNodeSaveAsset: () => {},
        onNodeMaskEdit: () => {}, onNodeEmotion: () => {}, onNodePortraitTexture: () => {}, onNodeCrop: () => {}, onNodeSplit: () => {}, onNodeUpscale: () => {},
        onNodeSuperResolve: () => {}, onNodeAngle: () => {}, onNodeViewImage: () => {}, onNodeExtractVideoFrames: () => {}, onNodeReversePrompt: () => {},
        onNodeToggleFreeResize: () => {}, onNodeToggleLocked: () => {}, onNodeCopyPrompt: () => {},
        onNodeSubtitles: () => {}, onNodeTimeline: () => {},
    } as ToolbarHandlers;

    const ctx: ToolContext = {
        selectedCount,
        selectedNodeTypes: new Set(),
        selectedVideoCount: 0,
        canvasTool,
        workspaceMode,
        isProjectLinked,
        canUndo,
        canRedo,
        extractingVideoFrame: false,
        mergingVideos: false,
        addPanelOpen: addOpen,
        appearancePanelOpen: appearanceOpen,
        settingsPanelOpen: settingsOpen,
        handlers,
    };

    const items = resolveToolbarEntries("main", ctx, prefs ?? defaultToolbarPrefs("main"));

    // 解析添加节点菜单命令——onClick 绑定到 runAddAction 以在执行后关闭面板
    const addNodeCommands = resolveAddNodeMenuCommands(ctx);
    const toCommand = (cmd: ResolvedAddNodeMenuCommand): CanvasCreateCommand => ({
        id: cmd.id,
        label: cmd.label,
        icon: cmd.icon,
        badge: cmd.badge,
        section: cmd.section,
        onClick: () => runAddAction(() => cmd.run(ctx)),
    });
    const createCommands = addNodeCommands.map(toCommand);

    return (
        <div ref={rootRef} data-canvas-no-zoom className="pointer-events-none absolute inset-x-[var(--canvas-inset-x)] bottom-[var(--canvas-inset-y)] z-[var(--z-toolbar)] flex justify-center">
            <AnimatePresence>
                {addOpen ? (
                    <AddNodeMenu
                        x={panelX}
                        theme={theme}
                        commands={createCommands}
                    />
                ) : null}
            </AnimatePresence>

            <FloatingDock ref={dockRef} items={items} magnify={false} className="canvas-floating-dock pointer-events-auto max-w-full" style={canvasDockStyle(theme)} />

            <AnimatePresence>
                {appearanceOpen ? (
                    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: aceternityMotion.duration.instant }} className="pointer-events-auto absolute bottom-[var(--canvas-dock-popover-offset)] z-[var(--dock-z-popover)] w-[224px] max-w-[calc(100vw-24px)] -translate-x-1/2" style={{ left: panelX || "50%" }}>
                        <CanvasOverlay theme={theme} className="overflow-hidden p-2.5" onWheel={(event) => event.stopPropagation()}>
                            <PanelHeading title={canvasT("videoCanvas.toolbar.appearance", "画布外观")} subtitle={canvasT("videoCanvas.toolbar.appearanceSubtitle", "调整整个创作空间")} theme={theme} />
                            <CanvasAppearanceControls
                                appearance={appearance}
                                backgroundMode={backgroundMode}
                                colorTheme={colorTheme}
                                theme={theme}
                                onAppearanceChange={onAppearanceChange}
                                onSaveAppearanceDefault={onSaveAppearanceDefault}
                                onBackgroundModeChange={onBackgroundModeChange}
                            />
                            <div className="mt-2.5 flex items-center justify-between gap-2 rounded-[var(--dock-item-radius-labeled)] px-2 py-1.5" style={{ background: theme.spatial.surface }}>
                                <span className="inline-flex min-w-0 items-center gap-1.5 text-[var(--fs-tiny)] font-medium"><Info className="size-3" />{canvasT("videoCanvas.toolbar.imageInfo", "图片信息")}</span>
                                <Switch size="small" checked={showImageInfo} onChange={onShowImageInfoChange} />
                            </div>
                        </CanvasOverlay>
                    </motion.div>
                ) : null}
            </AnimatePresence>

            <ToolbarSettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} toolbar="main" />
        </div>
    );
}

function AddNodeMenu({ x, theme, commands }: {
    x: number;
    theme: CanvasTheme;
    commands: CanvasCreateCommand[];
}) {
    return (
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: aceternityMotion.duration.instant }} className="pointer-events-auto absolute bottom-[var(--canvas-dock-popover-offset)] z-[var(--dock-z-popover)] w-[260px] max-w-[calc(100vw-24px)] -translate-x-1/2" style={{ left: x || "50%" }}>
            <CanvasOverlay theme={theme} className="overflow-hidden p-2" onWheel={(event) => event.stopPropagation()}>
                <CanvasCreateMenu commands={commands} />
            </CanvasOverlay>
        </motion.div>
    );
}

function PanelHeading({ title, subtitle, theme }: { title: string; subtitle: string; theme: CanvasTheme }) {
    return (
        <div className="mb-2 px-0.5">
            <span className="block text-[var(--fs-label)] font-medium">{title}</span>
            <span className="mt-0.5 block text-[var(--fs-micro)]" style={{ color: theme.node.muted }}>{subtitle}</span>
        </div>
    );
}

function getPanelX(dock: HTMLDivElement | null, target: HTMLElement) {
    if (!dock) return 0;
    const rootBox = dock.parentElement?.getBoundingClientRect() || dock.getBoundingClientRect();
    const box = target.getBoundingClientRect();
    return box.left - rootBox.left + box.width / 2;
}
