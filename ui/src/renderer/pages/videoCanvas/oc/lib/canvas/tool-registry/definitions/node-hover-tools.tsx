import { Captions, Clapperboard, Download, FolderPlus, GalleryHorizontalEnd, Image as ImageIcon, Info, LoaderCircle, Lock, Maximize2, MessageSquare, Minus, Music2, Plus, RefreshCw, Settings2, Trash2, Unlock, Upload, UserRound, Video } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { CONTENT_MODERATION_ERROR_CODE, isContentModerationError } from "@oc/lib/generation-error";
import { registerToolbarTools } from "../tool-registry";
import type { ToolContext, ToolDefinition } from "../tool-definition";
import { CanvasNodeType } from "@oc/types/canvas";

// 节点状态判定辅助函数——从 ToolContext 派生
function isImage(ctx: ToolContext) { return ctx.node?.type === CanvasNodeType.Image; }
function isVideo(ctx: ToolContext) { return ctx.node?.type === CanvasNodeType.Video; }
function isAudio(ctx: ToolContext) { return ctx.node?.type === CanvasNodeType.Audio; }
function isText(ctx: ToolContext) { return ctx.node?.type === CanvasNodeType.Text; }
function isConfig(ctx: ToolContext) { return ctx.node?.type === CanvasNodeType.Config; }
function hasImage(ctx: ToolContext) { return isImage(ctx) && Boolean(ctx.nodeMetadata?.content); }
function hasVideo(ctx: ToolContext) { return isVideo(ctx) && Boolean(ctx.nodeMetadata?.content); }
function hasAudio(ctx: ToolContext) { return isAudio(ctx) && Boolean(ctx.nodeMetadata?.content); }
function isCharacterReference(ctx: ToolContext) { return isText(ctx) && ctx.nodeMetadata?.workflowKind === "character" && Boolean(ctx.nodeMetadata?.characterAssetId); }
function isEditableText(ctx: ToolContext) { return isText(ctx) && !isCharacterReference(ctx); }
function canOpenDialog(ctx: ToolContext) { return isEditableText(ctx) || isImage(ctx) || isVideo(ctx); }
function canRetry(ctx: ToolContext) {
    const requiresPromptChange = ctx.nodeMetadata?.generationErrorCode === CONTENT_MODERATION_ERROR_CODE || isContentModerationError(ctx.nodeMetadata?.errorDetails);
    return ctx.nodeMetadata?.status === "error" && !requiresPromptChange;
}

export const nodeHoverToolbarTools: ToolDefinition[] = [
    // 基础工具组
    {
        id: "info",
        toolbar: "node-hover",
        category: "node-state",
        label: (ctx) => isCharacterReference(ctx) ? canvasT("videoCanvas.toolbar.characterDetailLong", "查看角色详情") : canvasT("videoCanvas.toolbar.infoDetail", "查看节点信息"),
        displayLabel: (ctx) => isCharacterReference(ctx) ? canvasT("videoCanvas.toolbar.characterDetail", "角色详情") : canvasT("videoCanvas.toolbar.info", "信息"),
        icon: (ctx) => isCharacterReference(ctx) ? <UserRound className="size-3.5" /> : <Info className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 10,
        run: (ctx) => ctx.handlers.onNodeInfo(ctx.node!),
    },
    {
        id: "delete",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.deleteLong", "移除节点"),
        displayLabel: () => canvasT("videoCanvas.toolbar.delete", "删除"),
        icon: <Trash2 className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 20,
        danger: true,
        run: (ctx) => ctx.handlers.onNodeDelete(ctx.node!),
    },
    // 节点操作工具组——通过 applicable 谓词实现上下文感知
    {
        id: "retry",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.retryLong", "重新生成"),
        displayLabel: () => canvasT("videoCanvas.toolbar.retry", "重试"),
        icon: <RefreshCw className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 30,
        applicable: canRetry,
        run: (ctx) => ctx.handlers.onNodeRetry(ctx.node!),
    },
    {
        id: "extractFrames",
        toolbar: "node-hover",
        category: "node-state",
        label: (ctx) => ctx.extractingVideoFrame ? canvasT("videoCanvas.toolbar.extractingFrameLong", "正在提取画面") : canvasT("videoCanvas.toolbar.extractFrameLong", "提取画面"),
        displayLabel: (ctx) => ctx.extractingVideoFrame ? canvasT("videoCanvas.toolbar.extractingFrame", "提取中") : canvasT("videoCanvas.toolbar.extractFrame", "画面"),
        icon: (ctx) => ctx.extractingVideoFrame ? <LoaderCircle className="size-3.5 animate-spin" /> : <GalleryHorizontalEnd className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 40,
        applicable: hasVideo,
        disabled: (ctx) => ctx.extractingVideoFrame,
        run: (ctx) => ctx.handlers.onNodeExtractVideoFrames(ctx.node!),
    },
    {
        id: "saveAsset",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.saveAssetLong", "加入我的素材"),
        displayLabel: () => canvasT("videoCanvas.toolbar.saveAsset", "存素材"),
        icon: <FolderPlus className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 50,
        applicable: (ctx) => hasImage(ctx) || hasVideo(ctx) || isEditableText(ctx),
        run: (ctx) => ctx.handlers.onNodeSaveAsset(ctx.node!),
    },
    {
        id: "download",
        toolbar: "node-hover",
        category: "node-state",
        label: (ctx) => hasAudio(ctx) ? canvasT("videoCanvas.toolbar.downloadAudio", "下载音频") : hasVideo(ctx) ? canvasT("videoCanvas.toolbar.downloadVideo", "下载视频") : canvasT("videoCanvas.toolbar.downloadImage", "下载图片"),
        displayLabel: () => canvasT("videoCanvas.toolbar.download", "下载"),
        icon: <Download className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 60,
        applicable: (ctx) => hasImage(ctx) || hasVideo(ctx) || hasAudio(ctx),
        run: (ctx) => ctx.handlers.onNodeDownload(ctx.node!),
    },
    {
        id: "edit",
        toolbar: "node-hover",
        category: "node-state",
        label: (ctx) => isEditableText(ctx) ? canvasT("videoCanvas.toolbar.textGenerateLong", "调用文本模型生成内容") : canvasT("videoCanvas.toolbar.edit", "编辑"),
        displayLabel: (ctx) => isEditableText(ctx) ? canvasT("videoCanvas.toolbar.textGenerate", "文本生成") : canvasT("videoCanvas.toolbar.edit", "编辑"),
        icon: <MessageSquare className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 70,
        applicable: canOpenDialog,
        run: (ctx) => ctx.handlers.onNodeToggleDialog(ctx.node!),
    },
    {
        id: "editText",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.expandEditLong", "放大编辑文本"),
        displayLabel: () => canvasT("videoCanvas.toolbar.expandEdit", "放大编辑"),
        icon: <Maximize2 className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 80,
        applicable: isEditableText,
        run: (ctx) => ctx.handlers.onNodeEditText(ctx.node!),
    },
    {
        id: "generateImage",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.genImageLong", "用文本生图"),
        displayLabel: () => canvasT("videoCanvas.toolbar.genImage", "生图"),
        icon: <ImageIcon className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 90,
        applicable: isEditableText,
        run: (ctx) => ctx.handlers.onNodeGenerateImage(ctx.node!),
    },
    {
        id: "config",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.genConfig", "生成配置"),
        displayLabel: () => canvasT("videoCanvas.toolbar.genConfig", "生成配置"),
        icon: <Settings2 className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 100,
        applicable: isConfig,
        run: (ctx) => ctx.handlers.onNodeToggleDialog(ctx.node!),
    },
    {
        id: "decreaseFont",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.fontDownLong", "减小字号"),
        displayLabel: () => canvasT("videoCanvas.toolbar.fontDown", "缩小"),
        icon: <Minus className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 110,
        applicable: isEditableText,
        run: (ctx) => ctx.handlers.onNodeDecreaseFont(ctx.node!),
    },
    {
        id: "increaseFont",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.fontUpLong", "增大字号"),
        displayLabel: () => canvasT("videoCanvas.toolbar.fontUp", "放大"),
        icon: <Plus className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 120,
        applicable: isEditableText,
        run: (ctx) => ctx.handlers.onNodeIncreaseFont(ctx.node!),
    },
    {
        id: "uploadImage",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.uploadImage", "上传图片"),
        displayLabel: () => canvasT("videoCanvas.toolbar.uploadImage", "上传图片"),
        icon: <Upload className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 130,
        applicable: (ctx) => isImage(ctx) && !hasImage(ctx),
        run: (ctx) => ctx.handlers.onNodeUpload(ctx.node!),
    },
    {
        id: "uploadVideo",
        toolbar: "node-hover",
        category: "node-state",
        label: (ctx) => hasVideo(ctx) ? canvasT("videoCanvas.toolbar.replaceVideo", "替换视频") : canvasT("videoCanvas.toolbar.uploadVideo", "上传视频"),
        displayLabel: (ctx) => hasVideo(ctx) ? canvasT("videoCanvas.toolbar.replaceVideo", "替换视频") : canvasT("videoCanvas.toolbar.uploadVideo", "上传视频"),
        icon: <Video className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 140,
        applicable: isVideo,
        run: (ctx) => ctx.handlers.onNodeUpload(ctx.node!),
    },
    {
        id: "subtitles",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.subtitlesLong", "编辑字幕"),
        displayLabel: () => canvasT("videoCanvas.toolbar.subtitles", "字幕"),
        icon: <Captions className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 145,
        applicable: (ctx) => hasVideo(ctx),
        run: (ctx) => ctx.handlers.onNodeSubtitles(ctx.node!),
    },
    {
        id: "timeline",
        toolbar: "node-hover",
        category: "node-state",
        label: () => canvasT("videoCanvas.toolbar.timelineLong", "时间线编辑"),
        displayLabel: () => canvasT("videoCanvas.toolbar.timeline", "时间线"),
        icon: <Clapperboard className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 148,
        applicable: (ctx) => hasVideo(ctx) || hasAudio(ctx),
        run: (ctx) => ctx.handlers.onNodeTimeline(ctx.node!),
    },
    {
        id: "uploadAudio",
        toolbar: "node-hover",
        category: "node-state",
        label: (ctx) => hasAudio(ctx) ? canvasT("videoCanvas.toolbar.replaceAudio", "替换音频") : canvasT("videoCanvas.toolbar.uploadAudio", "上传音频"),
        displayLabel: (ctx) => hasAudio(ctx) ? canvasT("videoCanvas.toolbar.replaceAudio", "替换音频") : canvasT("videoCanvas.toolbar.uploadAudio", "上传音频"),
        icon: <Music2 className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 150,
        applicable: isAudio,
        run: (ctx) => ctx.handlers.onNodeUpload(ctx.node!),
    },
    // 节点锁定——独立分类，自动插入前置分隔符
    {
        id: "node-lock",
        toolbar: "node-hover",
        category: "navigation",
        label: (ctx) => ctx.nodeMetadata?.locked ? canvasT("videoCanvas.toolbar.unlockLong", "解锁节点") : canvasT("videoCanvas.toolbar.lockLong", "锁定位置和尺寸"),
        displayLabel: (ctx) => ctx.nodeMetadata?.locked ? canvasT("videoCanvas.toolbar.unlock", "解锁") : canvasT("videoCanvas.toolbar.lock", "锁定"),
        icon: (ctx) => ctx.nodeMetadata?.locked ? <Unlock className="size-3.5" /> : <Lock className="size-3.5" />,
        defaultVisible: true,
        defaultOrder: 160,
        active: (ctx) => Boolean(ctx.nodeMetadata?.locked),
        run: (ctx) => ctx.handlers.onNodeToggleLocked(ctx.node!),
    },
];

registerToolbarTools(nodeHoverToolbarTools);
