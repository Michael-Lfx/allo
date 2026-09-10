import { useTranslation } from "react-i18next";
import { GripVertical, RotateCcw, SlidersHorizontal } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { FloatingDock, type FloatingDockEntry } from "@oc/components/ui/aceternity/floating-dock";
import { CanvasSheet, CanvasSheetButton } from "@oc/components/canvas/canvas-overlay";
import { canvasDockStyle } from "@oc/lib/canvas/canvas-aceternity-style";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { defaultToolbarPrefs, getToolbarTools, persistToolbarPrefs, readToolbarPrefs, type ToolbarId, type ToolbarPrefs, type ToolCategory, type ToolContext, type ToolDefinition } from "@oc/lib/canvas/tool-registry";
import { useThemeStore } from "@oc/stores/use-theme-store";

type ToolbarSettingsModalProps = {
    open: boolean;
    onClose: () => void;
    toolbar: ToolbarId;
};

/** 设置面板用的最小化上下文——仅用于解析工具的 label/icon */
const settingsMockContext: ToolContext = {
    selectedCount: 0,
    selectedNodeTypes: new Set(),
    selectedVideoCount: 0,
    canvasTool: "move",
    workspaceMode: "professional",
    isProjectLinked: false,
    canUndo: false,
    canRedo: false,
    extractingVideoFrame: false,
    mergingVideos: false,
    addPanelOpen: false,
    appearancePanelOpen: false,
    settingsPanelOpen: false,
    handlers: {} as ToolContext["handlers"],
};

type SettingsItem = {
    id: string;
    label: string;
    icon: ReactNode;
    visible: boolean;
    category: ToolCategory;
    danger?: boolean;
};

const CATEGORY_LABEL: Record<ToolCategory, [string, string]> = {
    navigation: ["videoCanvas.dialog.toolbarCategoryNavigation", "导航"],
    history: ["videoCanvas.dialog.toolbarCategoryHistory", "历史"],
    create: ["videoCanvas.dialog.toolbarCategoryCreate", "创建"],
    resource: ["videoCanvas.dialog.toolbarCategoryResource", "资源"],
    appearance: ["videoCanvas.dialog.toolbarCategoryAppearance", "外观与设置"],
    danger: ["videoCanvas.dialog.toolbarCategoryDanger", "危险操作"],
    layout: ["videoCanvas.dialog.toolbarCategoryLayout", "对齐"],
    arrange: ["videoCanvas.dialog.toolbarCategoryArrange", "排列"],
    selection: ["videoCanvas.dialog.toolbarCategorySelection", "多选"],
    "node-state": ["videoCanvas.dialog.toolbarCategoryNode", "节点"],
};

export function ToolbarSettingsModal({ open, onClose, toolbar }: ToolbarSettingsModalProps) {
    const { i18n } = useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const reducedMotion = useReducedMotion();
    const [items, setItems] = useState<SettingsItem[]>([]);
    const [toolbarId, setToolbarId] = useState<ToolbarId>(toolbar);
    const draggedItemIdRef = useRef<string | null>(null);
    const dragTargetIdRef = useRef<string | null>(null);
    const [draggedItemId, setDraggedItemId] = useState<string | null>(null);
    const visibleCount = items.filter((item) => item.visible).length;
    const previewItems = useMemo(() => buildPreviewEntries(items), [items]);

    useEffect(() => {
        if (!open) return;
        setToolbarId(toolbar);
        setItems(loadSettingsItems(toolbar));
    }, [open, toolbar, i18n.language]);

    const handleDragStart = (id: string) => {
        draggedItemIdRef.current = id;
        dragTargetIdRef.current = id;
        setDraggedItemId(id);
    };

    const handleDragEnter = (targetId: string) => {
        const sourceId = draggedItemIdRef.current;
        if (!sourceId || dragTargetIdRef.current === targetId) return;
        dragTargetIdRef.current = targetId;

        setItems((current) => {
            const sourceIndex = current.findIndex((item) => item.id === sourceId);
            const targetIndex = current.findIndex((item) => item.id === targetId);
            if (sourceIndex < 0 || targetIndex < 0) return current;

            const next = [...current];
            const [movedItem] = next.splice(sourceIndex, 1);
            next.splice(targetIndex, 0, movedItem);
            persistCurrent(next);
            return next;
        });
    };

    const handleDragEnd = () => {
        draggedItemIdRef.current = null;
        dragTargetIdRef.current = null;
        setDraggedItemId(null);
    };

    const handleToggleVisible = (id: string, visible: boolean) => {
        setItems((prev) => {
            if (!visible && prev.filter((item) => item.visible).length <= 1) return prev;
            const next = prev.map((item) => item.id === id ? { ...item, visible } : item);
            persistCurrent(next);
            return next;
        });
    };

    const handleReset = () => {
        persistToolbarPrefs(toolbarId, defaultToolbarPrefs(toolbarId));
        setItems(loadSettingsItems(toolbarId));
    };

    const persistCurrent = (currentItems: SettingsItem[]) => {
        const prefs: ToolbarPrefs = {
            order: currentItems.map((item) => item.id),
            hidden: currentItems.filter((item) => !item.visible).map((item) => item.id),
        };
        persistToolbarPrefs(toolbarId, prefs);
    };

    return (
        <CanvasSheet
            open={open}
            theme={theme}
            width={420}
            className="canvas-toolbar-settings-sheet"
            title={
                <span className="inline-flex items-center gap-2">
                    <SlidersHorizontal className="size-3.5" />
                    {canvasT("videoCanvas.dialog.toolbarSettings", "工具栏设置")}
                </span>
            }
            subtitle={canvasT("videoCanvas.dialog.toolbarSettingsHint", "预览底部工具栏，拖动排序，按需隐藏少用入口")}
            onClose={onClose}
            footer={
                <>
                    <CanvasSheetButton theme={theme} onClick={handleReset} aria-label={canvasT("videoCanvas.dialog.resetDefaultsAria", "恢复默认工具栏设置")}>
                        <RotateCcw className="size-3" />
                        {canvasT("videoCanvas.dialog.resetDefaults", "恢复默认")}
                    </CanvasSheetButton>
                    <CanvasSheetButton theme={theme} variant="primary" onClick={onClose}>
                        {canvasT("videoCanvas.dialog.done", "完成")}
                    </CanvasSheetButton>
                </>
            }
        >
            <div className="canvas-toolbar-settings-preview relative grid h-[88px] place-items-center overflow-hidden border-b" style={{ background: theme.canvas.background, borderColor: theme.toolbar.border }}>
                <div className="absolute inset-0 bg-[radial-gradient(currentColor_1px,transparent_1px)] opacity-[0.12] [background-size:16px_16px]" />
                <div className="pointer-events-none relative max-w-full overflow-x-auto px-4 py-3">
                    {previewItems.length > 0 ? (
                        <FloatingDock
                            items={previewItems}
                            size="compact"
                            magnify={false}
                            ariaLabel={canvasT("videoCanvas.dialog.toolbarPreviewAria", "底部工具栏预览")}
                            className="canvas-floating-dock"
                            style={canvasDockStyle(theme)}
                        />
                    ) : (
                        <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>
                            {canvasT("videoCanvas.dialog.toolbarPreviewEmpty", "至少保留一个工具")}
                        </span>
                    )}
                </div>
            </div>

            <div className="flex items-center justify-between gap-3 px-3 py-2">
                <span className="text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.muted }}>
                    {canvasT("videoCanvas.dialog.visibleCount", "已显示 {{visible}}/{{total}}", { visible: visibleCount, total: items.length })}
                </span>
            </div>

            <div className="px-2 pb-2" aria-label={canvasT("videoCanvas.dialog.mainToolbarOrderAria", "主工具栏顺序")}>
                {renderSettingsRows(items, {
                    reducedMotion: Boolean(reducedMotion),
                    theme,
                    draggedItemId,
                    visibleCount,
                    onToggleVisible: handleToggleVisible,
                    onDragStart: handleDragStart,
                    onDragEnter: handleDragEnter,
                    onDragEnd: handleDragEnd,
                })}
            </div>
        </CanvasSheet>
    );
}

function ToolbarSettingsItem({ item, reducedMotion, theme, dragging, lastVisible, onToggleVisible, onDragStart, onDragEnter, onDragEnd }: { item: SettingsItem; reducedMotion: boolean; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; dragging: boolean; lastVisible: boolean; onToggleVisible: (id: string, visible: boolean) => void; onDragStart: (id: string) => void; onDragEnter: (id: string) => void; onDragEnd: () => void }) {
    useTranslation();
    return (
        <motion.div
            layout={!reducedMotion}
            transition={reducedMotion ? { duration: 0 } : { duration: 0.18 }}
            className={`canvas-toolbar-settings-row flex h-10 min-w-0 items-center gap-1.5 rounded-[var(--r-lg)] px-1.5 ${item.visible ? "" : "is-hidden"} ${dragging ? "is-dragging" : ""}`}
            style={{ color: theme.node.text }}
            onDragEnter={() => onDragEnter(item.id)}
            onDragOver={(event) => event.preventDefault()}
        >
            <button
                type="button"
                draggable
                className="grid size-7 shrink-0 touch-none cursor-grab place-items-center rounded-[var(--r-md)] outline-none opacity-30 transition-opacity hover:opacity-70 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 active:cursor-grabbing"
                style={{ color: theme.node.muted, outlineColor: theme.accent.primary }}
                onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = "move";
                    onDragStart(item.id);
                }}
                onDragEnd={onDragEnd}
                aria-label={canvasT("videoCanvas.dialog.dragReorderAria", "拖动调整{{label}}顺序", { label: item.label })}
            >
                <GripVertical className="size-3.5" />
            </button>
            <span
                className="grid size-8 shrink-0 place-items-center rounded-[var(--dock-item-radius)] [&_svg]:size-4"
                style={{ background: theme.toolbar.itemHover, color: item.danger ? theme.accent.danger : theme.node.text }}
            >
                {item.icon}
            </span>
            <span className="min-w-0 flex-1 truncate text-[13px] font-medium leading-none" title={item.label}>{item.label}</span>
            <button
                type="button"
                role="switch"
                aria-checked={item.visible}
                disabled={lastVisible}
                className="canvas-toolbar-toggle"
                style={{ color: theme.node.muted, ["--dock-command-active" as string]: theme.accent.primary }}
                onClick={() => onToggleVisible(item.id, !item.visible)}
                aria-label={item.visible ? canvasT("videoCanvas.dialog.hideToolAria", "隐藏{{label}}", { label: item.label }) : canvasT("videoCanvas.dialog.showToolAria", "显示{{label}}", { label: item.label })}
            />
        </motion.div>
    );
}

function renderSettingsRows(items: SettingsItem[], props: {
    reducedMotion: boolean;
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    draggedItemId: string | null;
    visibleCount: number;
    onToggleVisible: (id: string, visible: boolean) => void;
    onDragStart: (id: string) => void;
    onDragEnter: (id: string) => void;
    onDragEnd: () => void;
}) {
    const rows: ReactNode[] = [];
    let previousCategory: ToolCategory | null = null;
    for (const item of items) {
        if (item.category !== previousCategory) {
            const [key, fallback] = CATEGORY_LABEL[item.category];
            rows.push(
                <div
                    key={`category-${item.category}-${item.id}`}
                    className="px-2 pb-1 pt-2.5 text-[10px] font-semibold uppercase tracking-[0.08em] first:pt-0.5"
                    style={{ color: props.theme.node.muted }}
                >
                    {canvasT(key, fallback)}
                </div>,
            );
            previousCategory = item.category;
        }
        rows.push(
            <ToolbarSettingsItem
                key={item.id}
                item={item}
                reducedMotion={props.reducedMotion}
                theme={props.theme}
                dragging={props.draggedItemId === item.id}
                lastVisible={item.visible && props.visibleCount <= 1}
                onToggleVisible={props.onToggleVisible}
                onDragStart={props.onDragStart}
                onDragEnter={props.onDragEnter}
                onDragEnd={props.onDragEnd}
            />,
        );
    }
    return rows;
}

function buildPreviewEntries(items: SettingsItem[]): FloatingDockEntry[] {
    const entries: FloatingDockEntry[] = [];
    let previousCategory: ToolCategory | null = null;
    let separatorIndex = 0;
    for (const item of items) {
        if (!item.visible) continue;
        if (previousCategory && previousCategory !== item.category) {
            entries.push({ kind: "separator", id: `preview-sep-${separatorIndex}` });
            separatorIndex += 1;
        }
        entries.push({
            id: item.id,
            label: item.label,
            icon: item.icon,
            danger: item.danger,
        });
        previousCategory = item.category;
    }
    return entries;
}

function loadSettingsItems(toolbar: ToolbarId): SettingsItem[] {
    const tools = getToolbarTools(toolbar);
    const prefs = readToolbarPrefs(toolbar) ?? defaultToolbarPrefs(toolbar);
    const hiddenSet = new Set(prefs.hidden);
    const orderIndex = new Map(prefs.order.map((id, index) => [id, index]));
    const sorted = [...tools].sort((a, b) => {
        const ai = orderIndex.has(a.id) ? orderIndex.get(a.id)! : Number.MAX_SAFE_INTEGER;
        const bi = orderIndex.has(b.id) ? orderIndex.get(b.id)! : Number.MAX_SAFE_INTEGER;
        if (ai !== bi) return ai - bi;
        return a.defaultOrder - b.defaultOrder;
    });
    return sorted.map((tool) => ({
        id: tool.id,
        label: resolveLabel(tool, settingsMockContext),
        icon: resolveIcon(tool, settingsMockContext),
        visible: !hiddenSet.has(tool.id),
        category: tool.category,
        danger: tool.danger,
    }));
}

function resolveLabel(tool: ToolDefinition, ctx: ToolContext): string {
    return typeof tool.label === "function" ? tool.label(ctx) : tool.label;
}

function resolveIcon(tool: ToolDefinition, ctx: ToolContext): ReactNode {
    return typeof tool.icon === "function" ? tool.icon(ctx) : tool.icon;
}
