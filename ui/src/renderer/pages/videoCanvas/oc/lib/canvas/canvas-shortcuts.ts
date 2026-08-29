import { canvasT } from "@oc/lib/canvas/canvas-i18n";

export type CanvasShortcutCategoryId = "common" | "navigation" | "selection" | "editing";

export type CanvasShortcutCategory = {
    id: CanvasShortcutCategoryId;
    label: string;
    description: string;
};

export type CanvasShortcutItem = {
    id: string;
    category: CanvasShortcutCategoryId;
    title: string;
    description: string;
    keys: string[][];
    keywords?: string[];
};

export const CANVAS_MODIFIER_KEY = "Ctrl / Cmd";

export function canvasShortcutCategories(): CanvasShortcutCategory[] {
    return [
        { id: "common", label: canvasT("videoCanvas.shortcuts.catCommon", "常用"), description: canvasT("videoCanvas.shortcuts.catCommonDesc", "搜索、保存与界面") },
        { id: "navigation", label: canvasT("videoCanvas.shortcuts.catNavigation", "视图与导航"), description: canvasT("videoCanvas.shortcuts.catNavigationDesc", "平移、缩放与定位") },
        { id: "selection", label: canvasT("videoCanvas.shortcuts.catSelection", "选择与连接"), description: canvasT("videoCanvas.shortcuts.catSelectionDesc", "批量选择和节点连接") },
        { id: "editing", label: canvasT("videoCanvas.shortcuts.catEditing", "编辑与文件"), description: canvasT("videoCanvas.shortcuts.catEditingDesc", "编辑、撤销与导入") },
    ];
}

export function canvasShortcuts(): CanvasShortcutItem[] {
    return [
        { id: "search", category: "common", title: canvasT("videoCanvas.shortcuts.search", "搜索并定位节点"), description: canvasT("videoCanvas.shortcuts.searchDesc", "按名称或内容搜索并定位节点"), keys: [[CANVAS_MODIFIER_KEY, "F"]], keywords: ["查找", "定位", "find", "search"] },
        { id: "shortcuts", category: "common", title: canvasT("videoCanvas.shortcuts.openShortcuts", "打开快捷键"), description: canvasT("videoCanvas.shortcuts.openShortcutsDesc", "查看画布中的键盘和鼠标操作"), keys: [["?"]], keywords: ["帮助", "说明", "help"] },
        { id: "save", category: "common", title: canvasT("videoCanvas.shortcuts.save", "保存画布布局和位置"), description: canvasT("videoCanvas.shortcuts.saveDesc", "保存当前画布布局和节点位置"), keys: [[CANVAS_MODIFIER_KEY, "S"]], keywords: ["存储", "save"] },
        { id: "focus", category: "common", title: canvasT("videoCanvas.shortcuts.focusToggle", "进入 / 退出专注模式"), description: canvasT("videoCanvas.shortcuts.focusDesc", "隐藏界面干扰，聚焦当前画布"), keys: [["Shift", CANVAS_MODIFIER_KEY, "F"]], keywords: ["沉浸", "全屏", "focus"] },
        { id: "pan", category: "navigation", title: canvasT("videoCanvas.shortcuts.pan", "平移视图"), description: canvasT("videoCanvas.shortcuts.panDesc", "在画布空白处拖动，或使用空格键与中键拖动"), keys: [[canvasT("videoCanvas.shortcuts.keyPan", "空白处拖动")], ["Space", canvasT("videoCanvas.shortcuts.keyDrag", "拖动")], [canvasT("videoCanvas.shortcuts.keyMiddleDrag", "中键拖动")]], keywords: ["移动", "画布", "pan"] },
        { id: "zoom-wheel", category: "navigation", title: canvasT("videoCanvas.shortcuts.zoom", "缩放画布"), description: canvasT("videoCanvas.shortcuts.zoomDesc", "以鼠标所在位置为中心缩放"), keys: [[canvasT("videoCanvas.shortcuts.keyWheel", "滚轮")]], keywords: ["放大", "缩小", "zoom"] },
        { id: "zoom-controls", category: "navigation", title: canvasT("videoCanvas.shortcuts.zoomPrecise", "精确调整缩放"), description: canvasT("videoCanvas.shortcuts.zoomPreciseDesc", "使用画布缩放滑杆调整比例"), keys: [[canvasT("videoCanvas.shortcuts.keyZoomSlider", "缩放滑杆")]], keywords: ["比例", "zoom"] },
        { id: "zoom-steps", category: "navigation", title: canvasT("videoCanvas.shortcuts.zoomSteps", "步进缩放画布"), description: canvasT("videoCanvas.shortcuts.zoomStepsDesc", "按固定步长放大或缩小画布"), keys: [[CANVAS_MODIFIER_KEY, "+"], [CANVAS_MODIFIER_KEY, "-"]], keywords: ["放大", "缩小", "zoom"] },
        { id: "zoom-presets", category: "navigation", title: canvasT("videoCanvas.shortcuts.zoomPresets", "100% / 适应全部 / 适应选择"), description: canvasT("videoCanvas.shortcuts.zoomPresetsDesc", "0/1 恢复 100%，2 适应画布，3 适应选择"), keys: [[CANVAS_MODIFIER_KEY, "0"], [CANVAS_MODIFIER_KEY, "1"], [CANVAS_MODIFIER_KEY, "2"], [CANVAS_MODIFIER_KEY, "3"]], keywords: ["100%", "适应", "居中", "缩放", "fit"] },
        { id: "box-select", category: "selection", title: canvasT("videoCanvas.shortcuts.boxSelect", "框选多个节点"), description: canvasT("videoCanvas.shortcuts.boxSelectDesc", "按住修饰键后拖动选区"), keys: [["Shift", canvasT("videoCanvas.shortcuts.keyDrag", "拖动")], [CANVAS_MODIFIER_KEY, canvasT("videoCanvas.shortcuts.keyDrag", "拖动")]], keywords: ["多选", "范围", "selection"] },
        { id: "box-select-tool", category: "selection", title: canvasT("videoCanvas.shortcuts.boxSelectToolTitle", "使用框选工具"), description: canvasT("videoCanvas.shortcuts.boxSelectTool", "框选多个节点，完成后自动回到「移动与选择」"), keys: [[canvasT("videoCanvas.shortcuts.keyBoxTool", "框选工具"), canvasT("videoCanvas.shortcuts.keyDrag", "拖动")]], keywords: ["工具栏", "多选", "selection"] },
        { id: "add-selection", category: "selection", title: canvasT("videoCanvas.shortcuts.addSelect", "追加选择节点"), description: canvasT("videoCanvas.shortcuts.addSelectDesc", "保留已有选择并加入更多节点"), keys: [["Shift", canvasT("videoCanvas.shortcuts.keyClick", "点击")], [CANVAS_MODIFIER_KEY, canvasT("videoCanvas.shortcuts.keyClick", "点击")]], keywords: ["多选", "添加", "selection"] },
        { id: "remove-selection", category: "selection", title: canvasT("videoCanvas.shortcuts.removeSelect", "移除选择节点"), description: canvasT("videoCanvas.shortcuts.removeSelectDesc", "从当前选择中移除点击或框选的节点"), keys: [["Alt", canvasT("videoCanvas.shortcuts.keyClickBox", "点击 / 框选")]], keywords: ["取消", "排除", "selection"] },
        { id: "select-all", category: "selection", title: canvasT("videoCanvas.shortcuts.selectAll", "全选节点"), description: canvasT("videoCanvas.shortcuts.selectAllDesc", "选择画布中的全部节点"), keys: [[CANVAS_MODIFIER_KEY, "A"]], keywords: ["全部", "select all"] },
        { id: "batch-connect", category: "selection", title: canvasT("videoCanvas.shortcuts.batchConnect", "批量连接节点"), description: canvasT("videoCanvas.shortcuts.batchConnectDesc", "为两个或更多已选节点进入批量连接模式"), keys: [["Alt", "L"]], keywords: ["连线", "连接", "link"] },
        { id: "copy", category: "editing", title: canvasT("videoCanvas.shortcuts.copy", "复制节点"), description: canvasT("videoCanvas.shortcuts.copyDesc", "复制当前选中的节点"), keys: [[CANVAS_MODIFIER_KEY, "C"]], keywords: ["copy"] },
        { id: "paste", category: "editing", title: canvasT("videoCanvas.shortcuts.paste", "粘贴节点或剪贴板内容"), description: canvasT("videoCanvas.shortcuts.copyPaste", "复制 / 粘贴节点，或粘贴剪切板文本/图片"), keys: [[CANVAS_MODIFIER_KEY, "V"]], keywords: ["剪贴板", "paste"] },
        { id: "delete", category: "editing", title: canvasT("videoCanvas.shortcuts.delete", "删除选中"), description: canvasT("videoCanvas.shortcuts.deleteDesc", "删除选中的节点或连线"), keys: [["Delete"], ["Backspace"]], keywords: ["移除", "delete"] },
        { id: "undo", category: "editing", title: canvasT("videoCanvas.shortcuts.undo", "撤销"), description: canvasT("videoCanvas.shortcuts.undoDesc", "撤销上一步画布编辑"), keys: [[CANVAS_MODIFIER_KEY, "Z"]], keywords: ["undo"] },
        { id: "redo", category: "editing", title: canvasT("videoCanvas.shortcuts.redo", "重做"), description: canvasT("videoCanvas.shortcuts.redoDesc", "恢复刚刚撤销的画布编辑"), keys: [[CANVAS_MODIFIER_KEY, "Shift", "Z"], [CANVAS_MODIFIER_KEY, "Y"]], keywords: ["恢复", "redo"] },
        { id: "escape", category: "editing", title: canvasT("videoCanvas.shortcuts.escape", "取消选择并关闭浮层"), description: canvasT("videoCanvas.shortcuts.escapeDesc", "取消选择、关闭浮层或退出专注模式"), keys: [["Esc"]], keywords: ["关闭", "退出", "cancel"] },
        { id: "import-media", category: "editing", title: canvasT("videoCanvas.shortcuts.dropMedia", "上传到画布"), description: canvasT("videoCanvas.shortcuts.dropMediaDesc", "将图片、视频或音频文件拖入画布"), keys: [[canvasT("videoCanvas.shortcuts.keyDrop", "拖入媒体")]], keywords: ["上传", "文件", "图片", "视频", "音频", "upload"] },
    ];
}

export function filterCanvasShortcuts(query: string, category?: CanvasShortcutCategoryId | "all") {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    const terms = normalizedQuery.split(/\s+/).filter(Boolean);
    const categories = canvasShortcutCategories();
    const shortcuts = canvasShortcuts();

    return shortcuts.filter((shortcut) => {
        if (category && category !== "all" && shortcut.category !== category) return false;
        if (!terms.length) return true;

        const categoryLabel = categories.find((entry) => entry.id === shortcut.category)?.label || "";
        const searchableText = [shortcut.title, shortcut.description, categoryLabel, shortcut.keys.flat().join(" "), ...(shortcut.keywords || [])]
            .join(" ")
            .toLocaleLowerCase();
        return terms.every((term) => searchableText.includes(term));
    });
}
