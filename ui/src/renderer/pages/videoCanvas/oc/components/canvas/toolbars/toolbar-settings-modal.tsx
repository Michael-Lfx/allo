import { useTranslation } from "react-i18next";
import { Modal, Switch } from "antd";
import { GripVertical, RotateCcw, X } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { defaultToolbarPrefs, getToolbarTools, persistToolbarPrefs, readToolbarPrefs, type ToolbarId, type ToolbarPrefs, type ToolContext, type ToolDefinition } from "@oc/lib/canvas/tool-registry";
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
    icon: React.ReactNode;
    visible: boolean;
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

    // 当 modal 打开或 toolbar / 语言变化时，加载工具列表与偏好
    useEffect(() => {
        if (!open) return;
        setToolbarId(toolbar);
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
        setItems(sorted.map((tool) => ({
            id: tool.id,
            label: resolveLabel(tool, settingsMockContext),
            icon: resolveIcon(tool, settingsMockContext),
            visible: !hiddenSet.has(tool.id),
        })));
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
            const next = prev.map((item) => item.id === id ? { ...item, visible } : item);
            persistCurrent(next);
            return next;
        });
    };

    const handleReset = () => {
        const defaults = defaultToolbarPrefs(toolbarId);
        const tools = getToolbarTools(toolbarId);
        const hiddenSet = new Set(defaults.hidden);
        setItems(tools.map((tool) => ({
            id: tool.id,
            label: resolveLabel(tool, settingsMockContext),
            icon: resolveIcon(tool, settingsMockContext),
            visible: !hiddenSet.has(tool.id),
        })));
        persistToolbarPrefs(toolbarId, defaults);
    };

    const persistCurrent = (currentItems: SettingsItem[]) => {
        const prefs: ToolbarPrefs = {
            order: currentItems.map((item) => item.id),
            hidden: currentItems.filter((item) => !item.visible).map((item) => item.id),
        };
        persistToolbarPrefs(toolbarId, prefs);
    };

    return (
        <Modal
            className="canvas-toolbar-settings-modal"
            open={open}
            onCancel={onClose}
            footer={null}
            closable={false}
            width={720}
            centered
            destroyOnClose
            styles={{
                container: { padding: 0, background: theme.spatial.elevated, border: 0, boxShadow: "none" },
                body: { padding: 0, background: theme.spatial.elevated },
            }}
        >
            <div className="flex items-start justify-between gap-4 px-5 pb-3 pt-5">
                <div className="min-w-0">
                    <h2 className="text-[var(--fs-heading)] font-semibold leading-none">{canvasT("videoCanvas.dialog.toolbarSettings", "工具栏设置")}</h2>
                    <p className="mt-2 text-[var(--fs-caption)] leading-none" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.toolbarSettingsHint", "拖动调整顺序，关闭不常用入口")}</p>
                </div>
                <button
                    type="button"
                    onClick={onClose}
                    className="grid size-8 shrink-0 place-items-center rounded-[var(--dock-item-radius)] outline-none transition-colors hover:bg-black/5 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 dark:hover:bg-white/8"
                    style={{ color: theme.node.muted, outlineColor: theme.accent.primary }}
                    aria-label={canvasT("videoCanvas.dialog.closeToolbarSettings", "关闭工具栏设置")}
                >
                    <X className="size-4" />
                </button>
            </div>
            <div className="flex items-center justify-between gap-3 px-5 pb-2 pt-1">
                <span className="text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.visibleCount", "已显示 {{visible}}/{{total}}", { visible: visibleCount, total: items.length })}</span>
                <button
                    type="button"
                    onClick={handleReset}
                    className="inline-flex h-7 items-center gap-1.5 rounded-[var(--dock-item-radius)] px-2 text-[var(--fs-tiny)] font-medium outline-none transition-colors hover:bg-black/5 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 dark:hover:bg-white/8"
                    style={{ color: theme.node.muted, outlineColor: theme.accent.primary }}
                    aria-label={canvasT("videoCanvas.dialog.resetDefaultsAria", "恢复默认工具栏设置")}
                >
                    <RotateCcw className="size-3" />
                    {canvasT("videoCanvas.dialog.resetDefaults", "恢复默认")}
                </button>
            </div>
            <div className="grid grid-cols-2 gap-2 px-5 pb-5 pt-2 md:grid-cols-5" aria-label={canvasT("videoCanvas.dialog.mainToolbarOrderAria", "主工具栏顺序")}>
                {items.map((item) => (
                    <ToolbarSettingsItem
                        key={item.id}
                        item={item}
                        reducedMotion={Boolean(reducedMotion)}
                        theme={theme}
                        dragging={draggedItemId === item.id}
                        onToggleVisible={handleToggleVisible}
                        onDragStart={handleDragStart}
                        onDragEnter={handleDragEnter}
                        onDragEnd={handleDragEnd}
                    />
                ))}
            </div>
        </Modal>
    );
}

function ToolbarSettingsItem({ item, reducedMotion, theme, dragging, onToggleVisible, onDragStart, onDragEnter, onDragEnd }: { item: SettingsItem; reducedMotion: boolean; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; dragging: boolean; onToggleVisible: (id: string, visible: boolean) => void; onDragStart: (id: string) => void; onDragEnter: (id: string) => void; onDragEnd: () => void }) {
    useTranslation();
    return (
        <motion.div
            layout={!reducedMotion}
            transition={reducedMotion ? { duration: 0 } : undefined}
            className={`canvas-toolbar-settings-card flex h-24 min-w-0 flex-col rounded-[var(--r-md)] p-3 ${item.visible ? "" : "is-hidden"} ${dragging ? "is-dragging" : ""}`}
            style={{ color: theme.node.text }}
            onDragEnter={() => onDragEnter(item.id)}
            onDragOver={(event) => event.preventDefault()}
        >
            <div className="flex items-center justify-between gap-2">
                <button
                    type="button"
                    draggable
                    className="grid size-6 touch-none cursor-grab place-items-center rounded-[var(--r-sm)] outline-none opacity-35 transition-opacity hover:opacity-70 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 active:cursor-grabbing"
                    style={{ color: theme.node.muted, outlineColor: theme.accent.primary }}
                    onDragStart={(event) => {
                        event.dataTransfer.effectAllowed = "move";
                        onDragStart(item.id);
                    }}
                    onDragEnd={onDragEnd}
                    aria-label={canvasT("videoCanvas.dialog.dragReorderAria", "拖动调整{{label}}顺序", { label: item.label })}
                >
                    <GripVertical className="size-4" />
                </button>
                <Switch size="small" checked={item.visible} onChange={(checked) => onToggleVisible(item.id, checked)} aria-label={item.visible ? canvasT("videoCanvas.dialog.hideToolAria", "隐藏{{label}}", { label: item.label }) : canvasT("videoCanvas.dialog.showToolAria", "显示{{label}}", { label: item.label })} />
            </div>
            <div className="canvas-toolbar-settings-card-content mt-auto flex min-w-0 flex-col items-center gap-1">
                <span className="grid size-8 shrink-0 place-items-center rounded-[var(--r-md)]" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}>
                    <span className="grid size-4 place-items-center [&_svg]:size-4">{item.icon}</span>
                </span>
                <span className="max-w-full whitespace-nowrap text-[var(--fs-caption)] font-medium leading-5" title={item.label}>{item.label}</span>
            </div>
        </motion.div>
    );
}

function resolveLabel(tool: ToolDefinition, ctx: ToolContext): string {
    return typeof tool.label === "function" ? tool.label(ctx) : tool.label;
}

function resolveIcon(tool: ToolDefinition, ctx: ToolContext): React.ReactNode {
    return typeof tool.icon === "function" ? tool.icon(ctx) : tool.icon;
}
