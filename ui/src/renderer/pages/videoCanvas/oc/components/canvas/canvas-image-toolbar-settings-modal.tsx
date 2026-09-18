import { useTranslation } from "react-i18next";
import { useMemo, type ReactNode } from "react";
import { Ellipsis, Settings2, Type } from "lucide-react";

import { FloatingDock, type FloatingDockEntry } from "@oc/components/ui/aceternity/floating-dock";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { canvasDockStyle } from "@oc/lib/canvas/canvas-aceternity-style";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { ImageQuickToolId } from "./canvas-image-toolbar-tools";
import { CanvasSheet, CanvasSheetButton, CanvasToggle } from "./canvas-overlay";

export type ImageToolbarSettingsTool = {
    id: ImageQuickToolId;
    title: string;
    label: string;
    icon: ReactNode;
    active?: boolean;
    danger?: boolean;
};

export function ImageToolSettingsModal({ open, tools, selectedIds, showLabels, onToggle, onShowLabelsChange, onCancel, onSave }: {
    open: boolean;
    tools: ImageToolbarSettingsTool[];
    selectedIds: ImageQuickToolId[];
    showLabels: boolean;
    onToggle: (id: ImageQuickToolId, visible: boolean) => void;
    onShowLabelsChange: (visible: boolean) => void;
    onCancel: () => void;
    onSave: () => void;
}) {
    useTranslation();
    const maxSelected = 7;
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const selected = useMemo(() => new Set(selectedIds), [selectedIds]);
    const selectedTools = tools.filter((tool) => selected.has(tool.id));
    const previewItems: FloatingDockEntry[] = [
        ...selectedTools.map((tool) => ({ id: tool.id, label: tool.title, displayLabel: tool.label, icon: tool.icon, active: tool.active, danger: tool.danger })),
        { id: "more", label: canvasT("videoCanvas.dialog.customNodeTools", "自定义节点工具"), displayLabel: canvasT("videoCanvas.nodeUi.more", "更多"), icon: <Ellipsis className="size-4" /> },
    ];

    return (
        <CanvasSheet
            open={open}
            theme={theme}
            width={520}
            title={
                <span className="inline-flex items-center gap-2">
                    <Settings2 className="size-3.5" />
                    {canvasT("videoCanvas.dialog.customNodeDock", "自定义节点 Dock")}
                </span>
            }
            onClose={onCancel}
            footer={
                <>
                    <CanvasSheetButton theme={theme} className="ml-auto" onClick={onCancel}>{canvasT("videoCanvas.dialog.cancel", "取消")}</CanvasSheetButton>
                    <CanvasSheetButton theme={theme} variant="primary" onClick={onSave}>{canvasT("videoCanvas.dialog.saveSettings", "保存设置")}</CanvasSheetButton>
                </>
            }
        >
            <div className="flex h-11 items-center justify-between gap-3 border-b px-1" style={{ borderColor: theme.toolbar.border }}>
                <span className="flex min-w-0 items-center gap-2">
                    <span className="grid size-7 shrink-0 place-items-center rounded-[var(--r-md)]" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}><Type className="size-3.5" /></span>
                    <span className="flex h-5 items-center text-xs font-medium leading-none">{canvasT("videoCanvas.dialog.showFunctionNames", "显示功能名")}</span>
                </span>
                <CanvasToggle checked={showLabels} onChange={onShowLabelsChange} theme={theme} ariaLabel={canvasT("videoCanvas.dialog.showDockLabelsAria", "显示节点 Dock 功能名")} />
            </div>
            <div className="relative grid h-[92px] place-items-center overflow-hidden border-b" style={{ background: theme.canvas.background, borderColor: theme.toolbar.border }}>
                <div className="absolute inset-0 bg-[radial-gradient(currentColor_1px,transparent_1px)] opacity-15 [background-size:18px_18px]" />
                <div className="thin-scrollbar relative flex max-w-full overflow-x-auto px-4 py-3">
                    <FloatingDock items={previewItems} size="compact" showLabels={showLabels} ariaLabel={canvasT("videoCanvas.dialog.imageToolsPreviewAria", "图片节点工具预览")} className="shrink-0" style={canvasDockStyle(theme, theme.node.text)} />
                </div>
            </div>
            <div className="px-1 py-3">
                <div className="mb-2 flex h-5 items-center justify-between">
                    <span className="text-xs font-semibold">{canvasT("videoCanvas.dialog.quickTools", "快捷工具")}</span>
                    <span className="rounded-full px-2 text-[var(--fs-tiny)] leading-5" style={{ background: theme.accent.primarySoft, color: theme.accent.primary }}>{selectedTools.length}/{maxSelected}</span>
                </div>
                <div className="grid w-full grid-cols-2 gap-1 sm:grid-cols-4" role="group" aria-label={canvasT("videoCanvas.dialog.quickTools", "快捷工具")}>
                    {tools.map((tool) => {
                        const checked = selected.has(tool.id);
                        const disabled = !checked && selectedTools.length >= maxSelected;
                        return (
                            <label key={tool.id} className={`flex h-8 min-w-0 items-center gap-1 rounded-[var(--r-md)] border px-1.5 transition-colors ${disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer"}`} style={{ background: checked ? theme.accent.primarySoft : "transparent", borderColor: checked ? theme.accent.primary : theme.toolbar.border, color: checked ? theme.accent.primary : theme.node.text }}>
                                <input
                                    type="checkbox"
                                    className="size-3.5 shrink-0 accent-current"
                                    checked={checked}
                                    disabled={disabled}
                                    onChange={(event) => onToggle(tool.id, event.target.checked)}
                                />
                                <span className="grid size-5 shrink-0 place-items-center rounded-[var(--r-sm)] [&_svg]:size-3" style={{ background: checked ? theme.accent.primary : theme.toolbar.itemHover, color: checked ? "#ffffff" : theme.node.muted }}>{tool.icon}</span>
                                <span className="min-w-0 truncate text-[var(--fs-tiny)] font-medium leading-none">{tool.label}</span>
                            </label>
                        );
                    })}
                </div>
            </div>
        </CanvasSheet>
    );
}
