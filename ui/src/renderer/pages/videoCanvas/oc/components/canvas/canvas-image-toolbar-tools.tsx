import type { ReactNode } from "react";
import { Brush, Camera, Copy, FileText, Grid2x2, Lock, LockOpen, Maximize2, PencilLine, Scissors, SlidersHorizontal, Smile, Sparkles, Upload, ZoomIn } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import type { CanvasNodeData } from "@oc/types/canvas";

export type ImageNodeActionToolId = "copyPrompt" | "reversePrompt" | "replace" | "resize" | "annotation" | "maskEdit" | "emotion" | "portraitTexture" | "crop" | "split" | "upscale" | "superResolve" | "angle" | "view";
export type ImageQuickToolId = "info" | "delete" | "saveAsset" | "download" | "edit" | ImageNodeActionToolId;

export type ImageToolHandlers = {
    onUpload: (node: CanvasNodeData) => void;
    onToggleFreeResize: (node: CanvasNodeData) => void;
    onAnnotate: (node: CanvasNodeData) => void;
    onMaskEdit: (node: CanvasNodeData) => void;
    onEmotion: (node: CanvasNodeData) => void;
    onPortraitTexture: (node: CanvasNodeData) => void;
    onCrop: (node: CanvasNodeData) => void;
    onSplit: (node: CanvasNodeData) => void;
    onUpscale: (node: CanvasNodeData) => void;
    onSuperResolve: (node: CanvasNodeData) => void;
    onAngle: (node: CanvasNodeData) => void;
    onViewImage: (node: CanvasNodeData) => void;
    onCopyPrompt: (node: CanvasNodeData) => void;
    onReversePrompt: (node: CanvasNodeData) => void;
};

export type ImageToolDefinition = {
    id: ImageNodeActionToolId;
    defaultVisible: boolean;
    panelLabel: string | (() => string);
    label: string | ((node: CanvasNodeData) => string);
    title: string | ((node: CanvasNodeData) => string);
    icon: (node: CanvasNodeData) => ReactNode;
    active?: (node: CanvasNodeData) => boolean;
    run: (node: CanvasNodeData, handlers: ImageToolHandlers) => void;
};

export const IMAGE_QUICK_TOOLS_STORAGE_KEY = "canvas-image-quick-tools-v13";

const defaultBaseToolIds: ImageQuickToolId[] = ["info", "delete", "saveAsset", "download", "edit"];

export const imageToolDefinitions: ImageToolDefinition[] = [
    {
        id: "copyPrompt",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.copyPrompt", "复制提示词"),
        label: () => canvasT("videoCanvas.imageTools.copyPrompt", "复制提示词"),
        title: () => canvasT("videoCanvas.imageTools.copyPromptTitle", "复制生成该图片的提示词"),
        icon: () => <Copy className="size-3.5" />,
        run: (node, handlers) => handlers.onCopyPrompt(node),
    },
    {
        id: "reversePrompt",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.reversePrompt", "反推提示词"),
        label: () => canvasT("videoCanvas.imageTools.reversePrompt", "反推提示词"),
        title: () => canvasT("videoCanvas.imageTools.reversePromptTitle", "创建反推提示词的文本和配置节点"),
        icon: () => <FileText className="size-3.5" />,
        run: (node, handlers) => handlers.onReversePrompt(node),
    },
    {
        id: "replace",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.replace", "替换图片"),
        label: () => canvasT("videoCanvas.imageTools.replace", "替换图片"),
        title: () => canvasT("videoCanvas.imageTools.replace", "替换图片"),
        icon: () => <Upload className="size-3.5" />,
        run: (node, handlers) => handlers.onUpload(node),
    },
    {
        id: "resize",
        defaultVisible: false,
        panelLabel: () => canvasT("videoCanvas.imageTools.lockRatio", "锁比例"),
        label: (node) => (node.metadata?.freeResize ? canvasT("videoCanvas.imageTools.freeRatio", "自由比例") : canvasT("videoCanvas.imageTools.lockRatio", "锁比例")),
        title: (node) => (node.metadata?.freeResize ? canvasT("videoCanvas.imageTools.freeRatioTitle", "切换为等比缩放") : canvasT("videoCanvas.imageTools.lockRatioTitle", "切换为自由比例")),
        icon: (node) => (node.metadata?.freeResize ? <LockOpen className="size-3.5" /> : <Lock className="size-3.5" />),
        active: (node) => Boolean(node.metadata?.freeResize),
        run: (node, handlers) => handlers.onToggleFreeResize(node),
    },
    {
        id: "annotation",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.annotation", "标注"),
        label: () => canvasT("videoCanvas.imageTools.annotation", "标注"),
        title: () => canvasT("videoCanvas.imageTools.annotationTitle", "在图片上绘制标记并保存为新节点"),
        icon: () => <PencilLine className="size-3.5" />,
        run: (node, handlers) => handlers.onAnnotate(node),
    },
    {
        id: "maskEdit",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.maskEdit", "局部编辑"),
        label: () => canvasT("videoCanvas.imageTools.maskEdit", "局部编辑"),
        title: () => canvasT("videoCanvas.imageTools.maskEditTitle", "添加蒙版遮罩后局部修改"),
        icon: () => <Brush className="size-3.5" />,
        run: (node, handlers) => handlers.onMaskEdit(node),
    },
    {
        id: "emotion",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.emotionPanel", "表情与情绪"),
        label: () => canvasT("videoCanvas.imageTools.emotion", "情绪"),
        title: () => canvasT("videoCanvas.imageTools.emotionTitle", "调整人物表情与情绪"),
        icon: () => <Smile className="size-3.5" />,
        run: (node, handlers) => handlers.onEmotion(node),
    },
    {
        id: "portraitTexture",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.portraitTexturePanel", "人物质感调节"),
        label: () => canvasT("videoCanvas.imageTools.portraitTexture", "人物质感"),
        title: () => canvasT("videoCanvas.imageTools.portraitTextureTitle", "调节人景融合、光影、皮肤、纹理与锐度"),
        icon: () => <SlidersHorizontal className="size-3.5" />,
        run: (node, handlers) => handlers.onPortraitTexture(node),
    },
    {
        id: "crop",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.crop", "剪裁"),
        label: () => canvasT("videoCanvas.imageTools.crop", "剪裁"),
        title: () => canvasT("videoCanvas.imageTools.cropTitle", "剪裁并生成新节点"),
        icon: () => <Scissors className="size-3.5" />,
        run: (node, handlers) => handlers.onCrop(node),
    },
    {
        id: "split",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.split", "切图"),
        label: () => canvasT("videoCanvas.imageTools.split", "切图"),
        title: () => canvasT("videoCanvas.imageTools.splitTitle", "按行列切分图片"),
        icon: () => <Grid2x2 className="size-3.5" />,
        run: (node, handlers) => handlers.onSplit(node),
    },
    {
        id: "upscale",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.upscale", "放大"),
        label: () => canvasT("videoCanvas.imageTools.upscale", "放大"),
        title: () => canvasT("videoCanvas.imageTools.upscaleTitle", "放大图片分辨率"),
        icon: () => <ZoomIn className="size-3.5" />,
        run: (node, handlers) => handlers.onUpscale(node),
    },
    {
        id: "superResolve",
        defaultVisible: false,
        panelLabel: () => canvasT("videoCanvas.imageTools.superResolve", "超分"),
        label: () => canvasT("videoCanvas.imageTools.superResolve", "超分"),
        title: () => canvasT("videoCanvas.imageTools.superResolveTitle", "AI 超分"),
        icon: () => <Sparkles className="size-3.5" />,
        run: (node, handlers) => handlers.onSuperResolve(node),
    },
    {
        id: "angle",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.angle", "多视角"),
        label: () => canvasT("videoCanvas.imageTools.angle", "多视角"),
        title: () => canvasT("videoCanvas.imageTools.angleTitle", "生成不同观察视角"),
        icon: () => <Camera className="size-3.5" />,
        run: (node, handlers) => handlers.onAngle(node),
    },
    {
        id: "view",
        defaultVisible: true,
        panelLabel: () => canvasT("videoCanvas.imageTools.view", "查看大图"),
        label: () => canvasT("videoCanvas.imageTools.view", "查看大图"),
        title: () => canvasT("videoCanvas.imageTools.viewTitle", "查看图片详情"),
        icon: () => <Maximize2 className="size-3.5" />,
        run: (node, handlers) => handlers.onViewImage(node),
    },
];

export const defaultImageQuickToolIds: ImageQuickToolId[] = ["info", "download", "maskEdit", "emotion", "portraitTexture", "crop", "angle"];

export function isImageQuickToolId(value: string): value is ImageQuickToolId {
    return defaultBaseToolIds.some((id) => id === value) || imageToolDefinitions.some((tool) => tool.id === value);
}

export function buildImageToolbarTools(node: CanvasNodeData, handlers: ImageToolHandlers) {
    return imageToolDefinitions.map((tool) => ({
        id: tool.id,
        label: resolveToolText(tool.label, node),
        title: resolveToolText(tool.title, node),
        icon: tool.icon(node),
        active: tool.active?.(node),
        onClick: () => tool.run(node, handlers),
    }));
}

export function normalizeImageQuickToolIds(value: unknown[]) {
    const allIds: ImageQuickToolId[] = [...defaultBaseToolIds, ...imageToolDefinitions.map((tool) => tool.id)];
    const ids = new Set(allIds);
    return allIds.filter((id) => value.includes(id) && ids.has(id));
}

export function readImageQuickToolsConfig(value: unknown): ImageQuickToolId[] {
    return Array.isArray(value) ? normalizeImageQuickToolIds(value).slice(0, 7) : defaultImageQuickToolIds;
}

function resolveToolText(value: string | ((node: CanvasNodeData) => string), node: CanvasNodeData) {
    return typeof value === "function" ? value(node) : value;
}
