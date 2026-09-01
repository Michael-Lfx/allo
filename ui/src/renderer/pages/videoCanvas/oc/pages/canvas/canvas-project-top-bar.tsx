import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Link, useNavigate } from "react-router-dom";
import { ArrowLeft, Bot, Clapperboard, Coins, Download, Focus, FolderKanban, Gauge, LayoutGrid, LoaderCircle, Menu, Pencil, Plus, Redo2, Search, Share2, Trash2, Undo2, Upload } from "lucide-react";
import { Button, Dropdown, Tooltip } from "antd";
import { useTranslation } from "react-i18next";

import { useWalletBalance } from "@oc/hooks/use-wallet-balance";
import type { CanvasContextSummary } from "@oc/lib/canvas/canvas-context-summary";
import type { CanvasShortDramaProgress } from "@oc/lib/canvas/canvas-short-drama";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
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
                    <Tooltip title={canvasT("videoCanvas.chrome.backToList", "返回画布列表")}>
                        <button
                            type="button"
                            className="grid size-9 shrink-0 place-items-center rounded-full transition hover:bg-black/5 focus-visible:outline-none focus-visible:ring-2 dark:hover:bg-white/10"
                            style={{ color: theme.node.text, background: theme.spatial.elevated, boxShadow: "0 8px 24px rgba(15,23,42,.08)", "--tw-ring-color": theme.accent.primary } as CSSProperties}
                            onClick={goCanvasList}
                            aria-label={canvasT("videoCanvas.chrome.backToList", "返回画布列表")}
                        >
                            <ArrowLeft className="size-4" />
                        </button>
                    </Tooltip>
                    <Dropdown
                        trigger={["click"]}
                        menu={{
                            items: [
                                { key: "projects", icon: <LayoutGrid className="size-4" />, label: canvasT("videoCanvas.chrome.projectList", "画布列表"), onClick: goCanvasList },
                                { type: "divider" },
                                { key: "new", icon: <Plus className="size-4" />, label: canvasT("videoCanvas.chrome.newCanvas", "新建画布"), onClick: onCreateProject },
                                { key: "delete", danger: true, icon: <Trash2 className="size-4" />, label: canvasT("videoCanvas.chrome.deleteCanvas", "删除当前画布"), onClick: onDeleteProject },
                                { type: "divider" },
                                { key: "import", icon: <Upload className="size-4" />, label: canvasT("videoCanvas.chrome.importMedia", "导入素材"), onClick: onImportImage },
                                { key: "export-project", icon: <Download className="size-4" />, label: canvasT("videoCanvas.chrome.exportProject", "导出工程"), disabled: exporting, onClick: onExportProject },
                                { key: "publish-tv", icon: <Share2 className="size-4" />, label: canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV"), disabled: publishing, onClick: onPublishTvShow },
                                { key: "search", icon: <Search className="size-4" />, label: <MenuLabel text={canvasT("videoCanvas.chrome.searchNodes", "搜索节点")} shortcut="⌘ F" />, onClick: onOpenSearch },
                                {
                                    key: "performance",
                                    icon: <Gauge className="size-4" />,
                                    label: canvasT("videoCanvas.chrome.mediaPerformance", "媒体性能"),
                                    children: [
                                        { key: "performance-auto", label: canvasT("videoCanvas.chrome.perfAuto", "自动性能"), onClick: () => onMediaPerformanceModeChange("auto") },
                                        { key: "performance-quality", label: canvasT("videoCanvas.chrome.perfQuality", "画质优先"), onClick: () => onMediaPerformanceModeChange("quality") },
                                        { key: "performance-fast", label: canvasT("videoCanvas.chrome.perfPerformance", "性能优先"), onClick: () => onMediaPerformanceModeChange("performance") },
                                    ],
                                },
                                { type: "divider" },
                                { key: "undo", disabled: !canUndo, icon: <Undo2 className="size-4" />, label: <MenuLabel text={canvasT("videoCanvas.chrome.undo", "撤销")} shortcut="⌘ Z" />, onClick: onUndo },
                                { key: "redo", disabled: !canRedo, icon: <Redo2 className="size-4" />, label: <MenuLabel text={canvasT("videoCanvas.chrome.redo", "重做")} shortcut="⌘ ⇧ Z / ⌘ Y" />, onClick: onRedo },
                            ],
                        }}
                    >
                        <button type="button" className="grid size-9 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10" style={{ color: theme.node.text }} aria-label={canvasT("videoCanvas.chrome.openMenu", "打开画布菜单")}>
                            <Menu className="size-5" />
                        </button>
                    </Dropdown>

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
                                <Tooltip title={canvasT("videoCanvas.chrome.rename", "重命名画布")}>
                                    <button type="button" className="grid size-7 shrink-0 place-items-center rounded-md opacity-60 transition hover:bg-black/5 hover:opacity-100 focus-visible:outline-none focus-visible:ring-2 dark:hover:bg-white/10" style={{ color: theme.node.text }} onClick={onStartTitleEditing} aria-label={canvasT("videoCanvas.chrome.rename", "重命名画布")}>
                                        <Pencil className="size-3.5" />
                                    </button>
                                </Tooltip>
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

                <div className="pointer-events-auto flex items-center gap-1.5">
                    <Button type="text" className="!hidden !h-10 !w-10 !min-w-10 !rounded-xl !p-0 lg:!inline-flex" style={{ color: theme.node.text }} icon={<Search className="size-4" />} onClick={onOpenSearch} aria-label={canvasT("videoCanvas.chrome.searchCanvasNodes", "搜索画布节点")} title={canvasT("videoCanvas.chrome.searchCanvasNodes", "搜索画布节点")} />
                    <Dropdown
                        trigger={["click"]}
                        menu={{
                            selectable: true,
                            selectedKeys: [mediaPerformanceMode],
                            onClick: ({ key }) => onMediaPerformanceModeChange(key as CanvasMediaPerformanceMode),
                            items: [
                                { key: "auto", label: canvasT("videoCanvas.chrome.perfAuto", "自动性能") },
                                { key: "quality", label: canvasT("videoCanvas.chrome.perfQuality", "画质优先") },
                                { key: "performance", label: canvasT("videoCanvas.chrome.perfPerformance", "性能优先") },
                            ],
                        }}
                    >
                        <Button type="text" className="!hidden !h-10 !w-10 !min-w-10 !rounded-xl !p-0 lg:!inline-flex" style={{ color: theme.node.text }} icon={<Gauge className="size-4" />} aria-label={canvasT("videoCanvas.chrome.mediaPerformanceMode", "媒体性能模式")} title={canvasT("videoCanvas.chrome.mediaPerformanceMode", "媒体性能模式")} />
                    </Dropdown>
                    {compactAgentStatus ? <CompactAgentStatus status={compactAgentStatus} onClick={onToggleAgent} /> : null}
                    {user && creditsEnabled ? (
                        <Link
                            to="/billing"
                            className="inline-flex h-9 min-w-[5.5rem] items-center justify-center gap-1.5 rounded-lg px-2.5 text-xs font-medium tabular-nums transition hover:bg-black/5 dark:hover:bg-white/10"
                            style={{ color: theme.node.text }}
                            title={canvasT("videoCanvas.chrome.credits", "查看积分明细")}
                        >
                            {refreshing && availableMicrocredits === null ? <LoaderCircle className="size-3.5 animate-spin opacity-60" /> : <Coins className="size-3.5" />}
                            <span>{availableMicrocredits === null ? "--" : (availableMicrocredits / 1_000_000).toLocaleString(i18n.language, { maximumFractionDigits: 3 })}</span>
                        </Link>
                    ) : null}
                    <Tooltip title={canvasT("videoCanvas.chrome.focusMode", "进入专注模式（⇧⌘F）")}>
                        <Button
                            type="text"
                            className="!h-10 !w-10 !min-w-10 !rounded-xl !p-0"
                            style={{ color: theme.node.text }}
                            icon={<Focus className="size-4" />}
                            onClick={onEnterFocusMode}
                            aria-label={canvasT("videoCanvas.chrome.focusMode", "进入专注模式（⇧⌘F）")}
                        />
                    </Tooltip>
                    {shortDramaGuide ? (
                        <Tooltip title={shortDramaGuide.collapsed ? canvasT("videoCanvas.chrome.shortDramaExpand", "展开短剧流程") : canvasT("videoCanvas.chrome.shortDramaCollapse", "收起短剧流程")}>
                            <Button
                                type="text"
                                className="!h-10 !rounded-xl !px-2.5 !font-medium"
                                style={{ color: theme.node.text, background: shortDramaGuide.collapsed ? undefined : theme.toolbar.activeBg }}
                                icon={<Clapperboard className="size-4" />}
                                onClick={handleShortDramaGuideToggle}
                                aria-label={canvasT("videoCanvas.chrome.shortDrama", "短剧流程")}
                            >
                                <span className="tabular-nums">{shortDramaGuide.progress.completedCount}/5</span>
                            </Button>
                        </Tooltip>
                    ) : null}
                    <span className="h-6 w-px" style={{ background: theme.toolbar.border }} />
                    <Tooltip title={canvasT("videoCanvas.chrome.exportProject", "导出工程")}>
                        <Button
                            type="text"
                            className="!h-10 !rounded-xl !px-3 !font-medium"
                            style={{ color: theme.node.text, background: theme.toolbar.panel }}
                            icon={<Download className="size-4" />}
                            loading={exporting}
                            onClick={onExportProject}
                            aria-label={canvasT("videoCanvas.chrome.exportProject", "导出工程")}
                        >
                            <span className="hidden lg:inline">{canvasT("videoCanvas.chrome.exportProject", "导出工程")}</span>
                        </Button>
                    </Tooltip>
                    <Tooltip title={canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV")}>
                        <Button
                            type="text"
                            className="!h-10 !rounded-xl !px-3 !font-medium"
                            style={{ color: theme.node.text, background: theme.toolbar.panel }}
                            icon={<Share2 className="size-4" />}
                            loading={publishing}
                            onClick={onPublishTvShow}
                            aria-label={canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV")}
                        >
                            <span className="hidden lg:inline">{canvasT("videoCanvas.chrome.publishTv", "发布到 Flowy TV")}</span>
                        </Button>
                    </Tooltip>
                    <Button
                        type="text"
                        className="!h-10 !rounded-xl !px-3 !font-medium"
                        style={{ background: agentOpen ? theme.toolbar.activeBg : theme.toolbar.panel, color: theme.node.text, boxShadow: "0 10px 30px rgba(28,25,23,.10)" }}
                        icon={<Bot className="size-4" />}
                        onClick={onToggleAgent}
                    >
                        Agent
                    </Button>
                </div>
            </div>
            <CanvasShortcutsModal open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
        </>
    );
}

function MenuLabel({ text, shortcut }: { text: string; shortcut: string }) {
    return (
        <span className="flex min-w-36 items-center justify-between gap-8">
            <span>{text}</span>
            <span className="text-xs opacity-45">{shortcut}</span>
        </span>
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
        <button type="button" className="flex h-10 items-center gap-2 rounded-xl px-3 text-sm font-medium transition hover:opacity-85" style={{ background: theme.toolbar.panel, color: theme.node.text, boxShadow: "0 10px 30px rgba(28,25,23,.10)" }} onClick={onClick} title={canvasT("videoCanvas.chrome.openLocalCodex", "打开本地 Codex 面板")}>
            <span className="size-2 rounded-full" style={{ background: dotColor }} />
            <span className="max-w-[180px] truncate">{label}</span>
        </button>
    );
}
