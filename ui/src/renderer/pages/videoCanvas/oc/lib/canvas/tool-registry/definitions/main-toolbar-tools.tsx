import { Eraser, FolderOpen, Hand, Palette, Plus, Redo2, Settings2, SquareDashedMousePointer, Trash2, Undo2, X } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";

import { registerToolbarTools } from "../tool-registry";
import type { ToolDefinition } from "../tool-definition";

export const mainToolbarTools: ToolDefinition[] = [
    {
        id: "tool-move",
        toolbar: "main",
        category: "navigation",
        label: (ctx) => ctx.canvasTool === "box-select"
            ? canvasT("videoCanvas.toolbar.moveSelect", "移动与选择")
            : ctx.selectedCount
                ? (ctx.selectedCount > 1
                    ? canvasT("videoCanvas.toolbar.deselectCount", "取消选择 {{count}} 个节点", { count: ctx.selectedCount })
                    : canvasT("videoCanvas.toolbar.deselect", "取消选择"))
                : canvasT("videoCanvas.toolbar.moveSelect", "移动与选择"),
        icon: (ctx) => ctx.canvasTool === "box-select" ? <Hand /> : ctx.selectedCount ? <X /> : <Hand />,
        defaultVisible: true,
        defaultOrder: 10,
        active: (ctx) => ctx.canvasTool === "move",
        run: (ctx) => {
            if (ctx.canvasTool !== "move") ctx.handlers.onToolChange("move");
            else ctx.handlers.onDeselect();
        },
    },
    {
        id: "tool-box-select",
        toolbar: "main",
        category: "navigation",
        label: () => canvasT("videoCanvas.toolbar.boxSelect", "框选"),
        icon: <SquareDashedMousePointer />,
        defaultVisible: true,
        defaultOrder: 20,
        active: (ctx) => ctx.canvasTool === "box-select",
        run: (ctx) => ctx.handlers.onToolChange(ctx.canvasTool === "box-select" ? "move" : "box-select"),
    },
    {
        id: "tool-undo",
        toolbar: "main",
        category: "history",
        label: () => canvasT("videoCanvas.toolbar.undo", "撤销"),
        icon: <Undo2 />,
        defaultVisible: true,
        defaultOrder: 30,
        disabled: (ctx) => !ctx.canUndo,
        run: (ctx) => ctx.handlers.onUndo(),
    },
    {
        id: "tool-redo",
        toolbar: "main",
        category: "history",
        label: () => canvasT("videoCanvas.toolbar.redo", "重做"),
        icon: <Redo2 />,
        defaultVisible: true,
        defaultOrder: 40,
        disabled: (ctx) => !ctx.canRedo,
        run: (ctx) => ctx.handlers.onRedo(),
    },
    {
        id: "tool-add",
        toolbar: "main",
        category: "create",
        label: () => canvasT("videoCanvas.toolbar.addNode", "添加节点"),
        icon: <Plus />,
        defaultVisible: true,
        defaultOrder: 50,
        expands: true,
        active: (ctx) => ctx.addPanelOpen,
        run: (ctx, event) => ctx.handlers.onToggleAddPanel(event!),
    },
    {
        id: "tool-assets",
        toolbar: "main",
        category: "resource",
        label: () => canvasT("videoCanvas.toolbar.assets", "素材空间"),
        icon: <FolderOpen />,
        defaultVisible: true,
        defaultOrder: 60,
        run: (ctx) => ctx.handlers.onOpenMyAssets(),
    },
    {
        id: "tool-style",
        toolbar: "main",
        category: "appearance",
        label: () => canvasT("videoCanvas.toolbar.appearance", "画布外观"),
        icon: <Palette />,
        defaultVisible: true,
        defaultOrder: 70,
        expands: true,
        active: (ctx) => ctx.appearancePanelOpen,
        run: (ctx, event) => ctx.handlers.onToggleAppearancePanel(event!),
    },
    {
        id: "tool-settings",
        toolbar: "main",
        category: "appearance",
        label: () => canvasT("videoCanvas.toolbar.settings", "工具栏设置"),
        icon: <Settings2 />,
        defaultVisible: true,
        defaultOrder: 80,
        expands: true,
        active: (ctx) => ctx.settingsPanelOpen,
        run: (ctx) => ctx.handlers.onToggleSettingsPanel(),
    },
    {
        id: "tool-delete",
        toolbar: "main",
        category: "danger",
        label: (ctx) => ctx.selectedCount > 1
            ? canvasT("videoCanvas.toolbar.deleteCount", "删除 {{count}} 个节点", { count: ctx.selectedCount })
            : canvasT("videoCanvas.toolbar.deleteSelected", "删除选中节点"),
        icon: <Trash2 />,
        defaultVisible: true,
        defaultOrder: 90,
        danger: true,
        applicable: (ctx) => ctx.selectedCount > 0,
        run: (ctx) => ctx.handlers.onDeleteSelected(),
    },
    {
        id: "tool-clear",
        toolbar: "main",
        category: "danger",
        label: () => canvasT("videoCanvas.toolbar.clearCanvas", "清空画布"),
        icon: <Eraser />,
        defaultVisible: true,
        defaultOrder: 100,
        danger: true,
        run: (ctx) => ctx.handlers.onClear(),
    },
];

registerToolbarTools(mainToolbarTools);
