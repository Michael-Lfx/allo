import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Link, useNavigate } from "react-router-dom";
import { ArrowLeft, Bot, Clapperboard, Coins, Download, Focus, FolderKanban, Gauge, LayoutGrid, LoaderCircle, Menu, Pencil, Plus, Redo2, Search, Share2, Trash2, Undo2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CanvasChromeButton, CanvasMenuRow, CanvasMenuSeparator, overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { anchoredOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { useWalletBalance } from "@oc/hooks/use-wallet-balance";
import type { CanvasContextSummary } from "@oc/lib/canvas/canvas-context-summary";
import type { CanvasShortDramaProgress } from "@oc/lib/canvas/canvas-short-drama";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes, type CanvasTheme } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import type { CanvasMediaPerformanceMode } from "@oc/types/canvas";
import { CanvasShortcutsModal } from "./canvas-shortcuts-modal";
import { VIDEO_CANVAS_LIBRARY_PATH } from "@renderer/pages/videoCanvas/routes";

type CanvasTopBarProps = {
    title: string;
    titleDraft: string;
    isTitleEditing: boolean;
    onTitleDraftChange: (value: string) => void;
    onStartTitleEditing: () => void;
    onFinishTitleEditing: () => void;
    onCancelTitleEditing: () => void;
    canUndo: boolean;
    canRedo: boolean;
    onCreateProject: () => void;
    onDeleteProject: () => void;
    onImportImage: () => void;
    onUndo: () => void;
    onRedo: () => void;
    agentOpen: boolean;
    compactAgentStatus?: { connected: boolean; enabled: boolean; activity: string };
    onToggleAgent: () => void;
    shortcutRequestNonce: number;
    mediaPerformanceMode: CanvasMediaPerformanceMode;
    onMediaPerformanceModeChange: (mode: CanvasMediaPerformanceMode) => void;
    onOpenSearch: () => void;
    projectContext?: CanvasContextSummary & { projectId: string; projectName: string };
    onEnterFocusMode: () => void;
    shortDramaGuide?: { progress: CanvasShortDramaProgress; collapsed: boolean; onToggle: () => void };
    onExportProject: () => void;
    onPublishTvShow: () => void;
    exporting: boolean;
    publishing: boolean;
};

export function CanvasTopBar({
    title,
    titleDraft,
    isTitleEditing,
    onTitleDraftChange,
    onStartTitleEditing,
    onFinishTitleEditing,
    onCancelTitleEditing,
    canUndo,
    canRedo,
    onCreateProject,
    onDeleteProject,
    onImportImage,
    onUndo,
    onRedo,
    agentOpen,
    compactAgentStatus,
    onToggleAgent,
    shortcutRequestNonce,
    mediaPerformanceMode,
    onMediaPerformanceModeChange,
    onOpenSearch,
    projectContext,
    onEnterFocusMode,
    shortDramaGuide,
    onExportProject,
    onPublishTvShow,
    exporting,
    publishing,
}: CanvasTopBarProps) {
    const { i18n } = useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const navigate = useNavigate();
    const user = useUserStore((state) => state.user);
    const creditsEnabled = useUserStore((state) => state.features.creditsEnabled);
    const { availableMicrocredits, refreshing } = useWalletBalance(user?.id, creditsEnabled);
    const titleRef = useRef<HTMLDivElement>(null);
    const [shortcutsOpen, setShortcutsOpen] = useState(false);
    const goCanvasList = () => navigate(VIDEO_CANVAS_LIBRARY_PATH);

    const handleShortDramaGuideToggle = () => {
        shortDramaGuide?.onToggle();
    };

    useEffect(() => {
        if (shortcutRequestNonce > 0) setShortcutsOpen(true);
    }, [shortcutRequestNonce]);

    useEffect(() => {
        if (!isTitleEditing) return;
        const close = (event: PointerEvent) => {
            if (!titleRef.current?.contains(event.target as Node)) onFinishTitleEditing();
        };
        document.addEventListener("pointerdown", close, true);
        return () => document.removeEventListener("pointerdown", close, true);
    }, [isTitleEditing, onFinishTitleEditing]);

    return (
        <>
            <div className="pointer-events-none absolute left-0 right-0 top-0 z-[var(--z-toolbar)] flex h-[var(--canvas-topbar-h)] items-center justify-between px-[var(--canvas-inset-x)]">
                <div className="pointer-events-auto flex min-w-0 items-center gap-2">
                    <CanvasChromeButton
                        className="is-icon"
                        style={{ color: theme.node.text }}
                        onClick={goCanvasList}
                        title={canvasT("videoCanvas.chrome.backToList", "返回画布列表")}
                        aria-label={canvasT("videoCanvas.chrome.backToList", "返回画布列表")}
                    >
                        <ArrowLeft className="size-4" />
                    </CanvasChromeButton>
                    <TopBarOverflowMenu
                        theme={theme}
                        canUndo={canUndo}
                        canRedo={canRedo}
                        exporting={exporting}
                        publishing={publishing}
                        mediaPerformanceMode={mediaPerformanceMode}
                        onGoList={goCanvasList}
                        onCreateProject={onCreateProject}
                        onDeleteProject={onDeleteProject}
                        onImportImage={onImportImage}
                        onExportProject={onExportProject}
                        onPublishTvShow={onPublishTvShow}
                        onOpenSearch={onOpenSearch}
                        onMediaPerformanceModeChange={onMediaPerformanceModeChange}
                        onUndo={onUndo}
                        onRedo={onRedo}
                    />

                    <div ref={titleRef} className="flex min-w-0 flex-col items-start" style={{ color: theme.node.text }}>
                        {isTitleEditing ? (
                            <input
                                autoFocus
                                size={canvasTitleInputSize(titleDraft)}
                                value={titleDraft}
                                onChange={(event) => onTitleDraftChange(event.target.value)}
                                onBlur={onFinishTitleEditing}
                                onKeyDown={(event) => {
                                    if (event.key === "Enter") onFinishTitleEditing();
                                    if (event.key === "Escape") onCancelTitleEditing();
                                }}
                                className="h-8 w-auto min-w-12 max-w-[min(280px,42vw)] appearance-none border-0 bg-transparent p-0 text-left text-base font-semibold tracking-normal outline-none ring-0 focus:outline-none focus:ring-0 focus-visible:outline-none focus-visible:ring-0"
                                style={{ color: theme.node.text, caretColor: theme.accent.primary, border: 0, boxShadow: "none", outline: "none" }}
                                aria-label={canvasT("videoCanvas.chrome.canvasName", "画布名称")}
                            />
                        ) : (
                            <div className="flex min-w-0 items-center gap-0.5">
                                <button
                                    type="button"
                                    className="max-w-[280px] truncate text-left text-base font-semibold tracking-normal transition-opacity hover:opacity-75"
                                    style={{ color: theme.node.text }}
                                    onClick={onStartTitleEditing}
                                    title={canvasT("videoCanvas.chrome.renameHint", "点击修改画布名称")}
                                >
                                    {title}
                                </button>
                                <CanvasChromeButton
                                    className="is-icon !size-7 opacity-60 hover:opacity-100"
                                    style={{ color: theme.node.text }}
                                    onClick={onStartTitleEditing}
                                    title={canvasT("videoCanvas.chrome.rename", "重命名画布")}
                                    aria-label={canvasT("videoCanvas.chrome.rename", "重命名画布")}
                                >
                                    <Pencil className="size-3.5" />
                                </CanvasChromeButton>
                            </div>
                        )}
                        {projectContext && !isTitleEditing ? (
                            <div className="mt-0.5 flex max-w-[360px] items-center gap-1.5 text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>
                                <Link to={VIDEO_CANVAS_LIBRARY_PATH} className="inline-flex min-w-0 items-center gap-1 hover:underline" title={canvasT("videoCanvas.chrome.backToProject", "返回项目：{{name}}", { name: projectContext.projectName })}>
                                    <FolderKanban className="size-3 shrink-0" />
                                    <span className="max-w-[120px] truncate">{projectContext.projectName}</span>
                                </Link>
                                <span aria-hidden>·</span>
                                <button type="button" className="min-w-0 truncate hover:underline" onClick={onOpenSearch} title={canvasT("videoCanvas.chrome.searchChapterShot", "搜索并定位章节或镜头")}>
                                    {projectContext.chapterLabel || canvasT("videoCanvas.chrome.nodeCount", "{{count}} 个节点", { count: projectContext.nodeCount })}
                                    {projectContext.shotLabel ? ` · ${projectContext.shotLabel}` : ""}
                                    {projectContext.selectedCount ? canvasT("videoCanvas.chrome.selectedCount", " · 已选 {{count}}", { count: projectContext.selectedCount }) : ""}
                                </button>
                            </div>
                        ) : null}
                    </div>
                </div>

                <div className="pointer-events-auto flex items-center gap-1">
                    <CanvasChromeButton className="is-icon hidden lg:inline-flex" style={{ color: theme.node.text }} onClick={onOpenSearch} aria-label={canvasT("videoCanvas.chrome.searchCanvasNodes", "搜索画布节点")} title={canvasT("videoCanvas.chrome.searchCanvasNodes", "搜索画布节点")}>
                        <Search className="size-3.5" />
                    </CanvasChromeButton>
                    {compactAgentStatus ? <CompactAgentStatus status={compactAgentStatus} onClick={onToggleAgent} /> : null}
                    {user && creditsEnabled ? (
                        <Link
                            to="/billing"
                            className="inline-flex h-7 min-w-[4.5rem] items-center justify-center gap-1 rounded-lg px-2 text-[var(--fs-label)] font-medium tabular-nums"
                            style={{ color: theme.node.text }}
                            title={canvasT("videoCanvas.chrome.credits", "查看积分明细")}
                        >
                            {refreshing && availableMicrocredits === null ? <LoaderCircle className="size-3.5 animate-spin opacity-60" /> : <Coins className="size-3.5" />}
                            <span>{availableMicrocredits === null ? "--" : (availableMicrocredits / 1_000_000).toLocaleString(i18n.language, { maximumFractionDigits: 3 })}</span>
                        </Link>
                    ) : null}
                    <CanvasChromeButton className="is-icon" style={{ color: theme.node.text }} onClick={onEnterFocusMode} title={canvasT("videoCanvas.chrome.focusMode", "进入专注模式（⇧⌘F）")} aria-label={canvasT("videoCanvas.chrome.focusMode", "进入专注模式（⇧⌘F）")}>
                        <Focus className="size-3.5" />
                    </CanvasChromeButton>
                    {shortDramaGuide ? (
                        <CanvasChromeButton
                            aria-pressed={!shortDramaGuide.collapsed}
                            style={{ color: theme.node.text }}
                            onClick={handleShortDramaGuideToggle}
                            title={shortDramaGuide.collapsed ? canvasT("videoCanvas.chrome.shortDramaExpand", "展开短剧流程") : canvasT("videoCanvas.chrome.shortDramaCollapse", "收起短剧流程")}
                            aria-label={canvasT("videoCanvas.chrome.shortDrama", "短剧流程")}
                        >
                            <Clapperboard className="size-3.5" />
                            <span className="tabular-nums">{shortDramaGuide.progress.completedCount}/5</span>
                        </CanvasChromeButton>
                    ) : null}
                    <CanvasChromeButton className="is-icon" disabled={exporting} style={{ color: theme.node.text }} onClick={onExportProject} title={canvasT("videoCanvas.chrome.exportProject", "导出工程")} aria-label={canvasT("videoCanvas.chrome.exportProject", "导出工程")}>
                        <Download className="size-3.5" />
                    </CanvasChromeButton>
                    <CanvasChromeButton className="is-icon" disabled={publishing} style={{ color: theme.node.text }} onClick={onPublishTvShow} title={canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV")} aria-label={canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV")}>
                        <Share2 className="size-3.5" />
                    </CanvasChromeButton>
                    <CanvasChromeButton aria-pressed={agentOpen} style={{ color: theme.node.text }} onClick={onToggleAgent} title="Agent" aria-label="Agent">
                        <Bot className="size-3.5" />
                    </CanvasChromeButton>
                </div>
            </div>
            <CanvasShortcutsModal open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
        </>
    );
}

function TopBarOverflowMenu({
    theme,
    canUndo,
    canRedo,
    exporting,
    publishing,
    mediaPerformanceMode,
    onGoList,
    onCreateProject,
    onDeleteProject,
    onImportImage,
    onExportProject,
    onPublishTvShow,
    onOpenSearch,
    onMediaPerformanceModeChange,
    onUndo,
    onRedo,
}: {
    theme: CanvasTheme;
    canUndo: boolean;
    canRedo: boolean;
    exporting: boolean;
    publishing: boolean;
    mediaPerformanceMode: CanvasMediaPerformanceMode;
    onGoList: () => void;
    onCreateProject: () => void;
    onDeleteProject: () => void;
    onImportImage: () => void;
    onExportProject: () => void;
    onPublishTvShow: () => void;
    onOpenSearch: () => void;
    onMediaPerformanceModeChange: (mode: CanvasMediaPerformanceMode) => void;
    onUndo: () => void;
    onRedo: () => void;
}) {
    const triggerRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [open, setOpen] = useState(false);
    const close = () => setOpen(false);
    const rect = useAnchoredOverlay(open, triggerRef, panelRef, close);
    const geometry = rect
        ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 260, placement: "bottomLeft", estimatedHeight: 420 })
        : null;
    const run = (action: () => void) => {
        close();
        action();
    };

    return (
        <>
            <CanvasChromeButton
                ref={triggerRef}
                className="is-icon"
                expanded={open}
                style={{ color: theme.node.text }}
                title={canvasT("videoCanvas.chrome.openMenu", "打开画布菜单")}
                aria-label={canvasT("videoCanvas.chrome.openMenu", "打开画布菜单")}
                onClick={() => setOpen((value) => !value)}
            >
                <Menu className="size-4" />
            </CanvasChromeButton>
            {open && geometry
                ? createPortal(
                    <div ref={panelRef} className="canvas-overlay" style={overlayPanelStyle(theme, geometry)} onPointerDown={(event) => event.stopPropagation()}>
                        <CanvasMenuRow icon={<LayoutGrid className="size-3.5" />} label={canvasT("videoCanvas.chrome.projectList", "画布列表")} onClick={() => run(onGoList)} />
                        <CanvasMenuSeparator />
                        <CanvasMenuRow icon={<Plus className="size-3.5" />} label={canvasT("videoCanvas.chrome.newCanvas", "新建画布")} onClick={() => run(onCreateProject)} />
                        <CanvasMenuRow icon={<Trash2 className="size-3.5" />} label={canvasT("videoCanvas.chrome.deleteCanvas", "删除当前画布")} danger onClick={() => run(onDeleteProject)} />
                        <CanvasMenuSeparator />
                        <CanvasMenuRow icon={<Upload className="size-3.5" />} label={canvasT("videoCanvas.chrome.importMedia", "导入素材")} onClick={() => run(onImportImage)} />
                        <CanvasMenuRow icon={<Download className="size-3.5" />} label={canvasT("videoCanvas.chrome.exportProject", "导出工程")} disabled={exporting} onClick={() => run(onExportProject)} />
                        <CanvasMenuRow icon={<Share2 className="size-3.5" />} label={canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV")} disabled={publishing} onClick={() => run(onPublishTvShow)} />
                        <CanvasMenuRow icon={<Search className="size-3.5" />} label={canvasT("videoCanvas.chrome.searchNodes", "搜索节点")} shortcut="⌘F" onClick={() => run(onOpenSearch)} />
                        <CanvasMenuSeparator />
                        <CanvasMenuRow icon={<Gauge className="size-3.5" />} label={canvasT("videoCanvas.chrome.perfAuto", "自动性能")} active={mediaPerformanceMode === "auto"} onClick={() => run(() => onMediaPerformanceModeChange("auto"))} />
                        <CanvasMenuRow label={canvasT("videoCanvas.chrome.perfQuality", "画质优先")} active={mediaPerformanceMode === "quality"} onClick={() => run(() => onMediaPerformanceModeChange("quality"))} />
                        <CanvasMenuRow label={canvasT("videoCanvas.chrome.perfPerformance", "性能优先")} active={mediaPerformanceMode === "performance"} onClick={() => run(() => onMediaPerformanceModeChange("performance"))} />
                        <CanvasMenuSeparator />
                        <CanvasMenuRow icon={<Undo2 className="size-3.5" />} label={canvasT("videoCanvas.chrome.undo", "撤销")} shortcut="⌘Z" disabled={!canUndo} onClick={() => run(onUndo)} />
                        <CanvasMenuRow icon={<Redo2 className="size-3.5" />} label={canvasT("videoCanvas.chrome.redo", "重做")} shortcut="⌘⇧Z" disabled={!canRedo} onClick={() => run(onRedo)} />
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}

function canvasTitleInputSize(value: string) {
    const visualLength = Array.from(value || canvasT("videoCanvas.chrome.canvasName", "画布名称")).reduce((length, character) => length + (character.codePointAt(0)! > 0xff ? 2 : 1), 0);
    return Math.min(30, Math.max(5, visualLength));
}

function CompactAgentStatus({ status, onClick }: { status: { connected: boolean; enabled: boolean; activity: string }; onClick: () => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const label = status.connected
        ? canvasT("videoCanvas.chrome.localCodexConnected", "已连接到本地 Codex")
        : status.enabled
            ? status.activity || canvasT("videoCanvas.chrome.localCodexBusy", "连接中")
            : canvasT("videoCanvas.chrome.localCodexConnecting", "正在连接本地 Codex");
    const dotColor = status.connected ? "#22c55e" : status.enabled ? "#f59e0b" : theme.node.muted;
    return (
        <CanvasChromeButton style={{ color: theme.node.text }} onClick={onClick} title={canvasT("videoCanvas.chrome.openLocalCodex", "打开本地 Codex 面板")}>
            <span className="size-2 rounded-full" style={{ background: dotColor }} />
            <span className="max-w-[180px] truncate">{label}</span>
        </CanvasChromeButton>
    );
}
