import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { App, Button, Input, Modal, Segmented, Tag } from "antd";
import { ChevronDown, Ellipsis, Lock, Plus, Settings2, Unlock } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ASSET_CATEGORY_OPTIONS } from "@oc/lib/asset-category";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { canvasDockStyle } from "@oc/lib/canvas/canvas-aceternity-style";
import { defaultToolbarPrefs, readToolbarPrefs, resolveToolbarTools, type ToolContext, type ToolbarHandlers } from "@oc/lib/canvas/tool-registry";
import { subscribeCanvasViewportPreview } from "@oc/lib/canvas/canvas-live-viewport";
import { canvasNodeAssetCategory } from "@oc/lib/canvas/canvas-node-asset";
import { anchoredOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { getNodeLabel } from "@oc/lib/canvas/node-registry";
import { formatBytes, getDataUrlByteSize } from "@oc/lib/image-utils";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { CONTENT_MODERATION_ERROR_CODE, isContentModerationError } from "@oc/lib/generation-error";
import { useCopyText } from "@oc/hooks/use-copy-text";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasNodeType, type CanvasNodeData, type CanvasNodeMetadata, type CanvasWorkspaceMode, type ViewportTransform } from "@oc/types/canvas";
import { CanvasMenuRow, overlayPanelStyle, useAnchoredOverlay } from "./canvas-overlay";
import { ImageToolSettingsModal } from "./canvas-image-toolbar-settings-modal";
import { IMAGE_QUICK_TOOLS_STORAGE_KEY, buildImageToolbarTools, defaultImageQuickToolIds, isImageQuickToolId, readImageQuickToolsConfig, type ImageQuickToolId } from "./canvas-image-toolbar-tools";

type CanvasNodeToolbarProps = {
    node: CanvasNodeData | null;
    viewport: ViewportTransform;
    containerRef: RefObject<HTMLDivElement | null>;
    onKeep: (nodeId: string) => void;
    onLeave: () => void;
    onInfo: (node: CanvasNodeData) => void;
    onEditText: (node: CanvasNodeData) => void;
    onDecreaseFont: (node: CanvasNodeData) => void;
    onIncreaseFont: (node: CanvasNodeData) => void;
    onToggleDialog: (node: CanvasNodeData) => void;
    onAnnotate: (node: CanvasNodeData) => void;
    onGenerateImage: (node: CanvasNodeData) => void;
    onUpload: (node: CanvasNodeData) => void;
    onDownload: (node: CanvasNodeData) => void;
    onSaveAsset: (node: CanvasNodeData) => void;
    onMaskEdit: (node: CanvasNodeData) => void;
    onEmotion: (node: CanvasNodeData) => void;
    onPortraitTexture: (node: CanvasNodeData) => void;
    onCrop: (node: CanvasNodeData) => void;
    onSplit: (node: CanvasNodeData) => void;
    onUpscale: (node: CanvasNodeData) => void;
    onSuperResolve: (node: CanvasNodeData) => void;
    onAngle: (node: CanvasNodeData) => void;
    onViewImage: (node: CanvasNodeData) => void;
    onExtractVideoFrames: (node: CanvasNodeData) => void;
    extractingVideoFrame: boolean;
    onReversePrompt: (node: CanvasNodeData) => void;
    onRetry: (node: CanvasNodeData) => void;
    onToggleFreeResize: (node: CanvasNodeData) => void;
    onToggleLocked: (node: CanvasNodeData) => void;
    onSubtitles: (node: CanvasNodeData) => void;
    onTimeline: (node: CanvasNodeData) => void;
    onDelete: (node: CanvasNodeData) => void;
    workspaceMode?: CanvasWorkspaceMode;
};

type CanvasAssetCategory = NonNullable<NonNullable<CanvasNodeData["metadata"]>["assetCategory"]>;

const assetCategoryOptions = ASSET_CATEGORY_OPTIONS;

type ToolbarTool = {
    id: string;
    title: string;
    label: string;
    icon: ReactNode;
    onClick: () => void;
    active?: boolean;
    danger?: boolean;
    disabled?: boolean;
};

const MAX_IMAGE_QUICK_TOOLS = 7;
const NODE_DOCK_LABELS_STORAGE_KEY = "canvas-node-dock-show-labels-v1";

export function CanvasNodeToolbar({
    node,
    viewport,
    containerRef,
    onKeep,
    onLeave,
    onInfo,
    onEditText,
    onDecreaseFont,
    onIncreaseFont,
    onToggleDialog,
    onAnnotate,
    onGenerateImage,
    onUpload,
    onDownload,
    onSaveAsset,
    onMaskEdit,
    onEmotion,
    onPortraitTexture,
    onCrop,
    onSplit,
    onUpscale,
    onSuperResolve,
    onAngle,
    onViewImage,
    onExtractVideoFrames,
    extractingVideoFrame,
    onReversePrompt,
    onRetry,
    onToggleFreeResize,
    onToggleLocked,
    onSubtitles,
    onTimeline,
    onDelete,
    workspaceMode = "professional",
}: CanvasNodeToolbarProps) {
    useTranslation();
    const [quickImageToolIds, setQuickImageToolIds] = useState<ImageQuickToolId[]>(defaultImageQuickToolIds);
    const [draftImageToolIds, setDraftImageToolIds] = useState<ImageQuickToolId[]>(defaultImageQuickToolIds);
    const [showDockLabels, setShowDockLabels] = useState(() => {
        try {
            return window.localStorage.getItem(NODE_DOCK_LABELS_STORAGE_KEY) === "1";
        } catch {
            return false;
        }
    });
    const [draftShowDockLabels, setDraftShowDockLabels] = useState(false);
    const [imageToolSettingsOpen, setImageToolSettingsOpen] = useState(false);
    const [openMenuId, setOpenMenuId] = useState<string | null>(null);
    const [anchor, setAnchor] = useState<{ left: number; top: number } | null>(null);
    const toolbarRef = useRef<HTMLDivElement>(null);
    const imageToolSettingsOpenRef = useRef(false);
    const { message } = App.useApp();
    const copyText = useCopyText();
    const themeName = useThemeStore((state) => state.theme);
    const theme = canvasThemes[themeName];

    useEffect(() => {
        try {
            const stored = window.localStorage.getItem(IMAGE_QUICK_TOOLS_STORAGE_KEY);
            if (!stored) return;
            const parsed = JSON.parse(stored) as unknown;
            setQuickImageToolIds(readImageQuickToolsConfig(parsed));
        } catch {
            window.localStorage.removeItem(IMAGE_QUICK_TOOLS_STORAGE_KEY);
        }
    }, []);

    useEffect(() => {
        imageToolSettingsOpenRef.current = false;
        setImageToolSettingsOpen(false);
        setOpenMenuId(null);
    }, [node?.id]);

    useLayoutEffect(() => {
        const container = containerRef.current;
        if (!node || !container) {
            setAnchor((current) => (current === null ? current : null));
            return;
        }
        const element = container.querySelector<HTMLElement>(`[data-node-id="${CSS.escape(node.id)}"]`);
        if (!element) {
            setAnchor((current) => (current === null ? current : null));
            return;
        }
        const update = () => {
            const nodeRect = element.getBoundingClientRect();
            const containerRect = container.getBoundingClientRect();
            const preferredLeft = nodeRect.left - containerRect.left + nodeRect.width / 2;
            const toolbarWidth = toolbarRef.current?.offsetWidth || 0;
            const halfToolbar = toolbarWidth / 2;
            const canClamp = toolbarWidth > 0 && toolbarWidth <= containerRect.width - 20;
            const left = Math.round(canClamp ? Math.min(Math.max(preferredLeft, halfToolbar + 10), containerRect.width - halfToolbar - 10) : preferredLeft);
            // 外置标题固定占 24px，额外保留 6px 即可兼顾层级和名称编辑入口。
            const top = Math.round(nodeRect.top - containerRect.top - 30);
            if (toolbarRef.current) {
                toolbarRef.current.style.left = `${left}px`;
                toolbarRef.current.style.top = `${top}px`;
            }
            setAnchor((current) => (current?.left === left && current.top === top ? current : { left, top }));
        };
        update();
        const resizeObserver = new ResizeObserver(update);
        resizeObserver.observe(element);
        resizeObserver.observe(container);
        if (toolbarRef.current) resizeObserver.observe(toolbarRef.current);
        const viewportLayer = element.parentElement;
        const mutationObserver = new MutationObserver(update);
        if (viewportLayer) mutationObserver.observe(viewportLayer, { attributes: true, attributeFilter: ["style"] });
        const unsubscribeViewport = subscribeCanvasViewportPreview(container, update);
        window.addEventListener("resize", update);
        return () => {
            resizeObserver.disconnect();
            mutationObserver.disconnect();
            unsubscribeViewport();
            window.removeEventListener("resize", update);
        };
    }, [containerRef, node, showDockLabels, viewport.k, viewport.x, viewport.y]);

    if (!node || !anchor) return null;

    const activeNode = node;
    const isImage = node.type === CanvasNodeType.Image;
    const isVideo = node.type === CanvasNodeType.Video;
    const isAudio = node.type === CanvasNodeType.Audio;
    const hasImage = isImage && Boolean(node.metadata?.content);
    const hasVideo = isVideo && Boolean(node.metadata?.content);
    const hasAudio = isAudio && Boolean(node.metadata?.content);
    const isText = node.type === CanvasNodeType.Text;
    const isCharacterReference = isText && node.metadata?.workflowKind === "character" && Boolean(node.metadata.characterAssetId);
    const isEditableText = isText && !isCharacterReference;
    const isConfig = node.type === CanvasNodeType.Config;
    const canOpenDialog = isEditableText || isImage || isVideo;
    const requiresPromptChange = node.metadata?.generationErrorCode === CONTENT_MODERATION_ERROR_CODE || isContentModerationError(node.metadata?.errorDetails);
    const canRetry = node.metadata?.status === "error" && !requiresPromptChange;
    const quickImageToolIdSet = new Set(quickImageToolIds);
    const copyImagePrompt = (target: CanvasNodeData) => {
        const prompt = target.metadata?.prompt?.trim();
        if (!prompt) {
            message.warning(canvasT("videoCanvas.nodeUi.noPromptToCopy", "暂无可复制的提示词"));
            return;
        }
        copyText(prompt, canvasT("videoCanvas.nodeUi.promptCopied", "提示词已复制"));
    };
    const imageTools = buildImageToolbarTools(node, { onUpload, onToggleFreeResize, onAnnotate, onMaskEdit, onEmotion, onPortraitTexture, onCrop, onSplit, onUpscale, onSuperResolve, onAngle, onViewImage, onCopyPrompt: copyImagePrompt, onReversePrompt });

    function openImageToolSettings() {
        imageToolSettingsOpenRef.current = true;
        onKeep(activeNode.id);
        setDraftImageToolIds(quickImageToolIds);
        setDraftShowDockLabels(showDockLabels);
        setImageToolSettingsOpen(true);
    }

    // 构建 ToolContext——供注册表解析工具
    const nodeHoverHandlers = {
        onNodeInfo: onInfo, onNodeDelete: onDelete, onNodeRetry: onRetry, onNodeEditText: onEditText, onNodeDecreaseFont: onDecreaseFont, onNodeIncreaseFont: onIncreaseFont,
        onNodeToggleDialog: onToggleDialog, onNodeAnnotate: onAnnotate, onNodeGenerateImage: onGenerateImage, onNodeUpload: onUpload, onNodeDownload: onDownload,
        onNodeSaveAsset: onSaveAsset, onNodeMaskEdit: onMaskEdit, onNodeEmotion: onEmotion, onNodePortraitTexture: onPortraitTexture, onNodeCrop: onCrop,
        onNodeSplit: onSplit, onNodeUpscale: onUpscale, onNodeSuperResolve: onSuperResolve, onNodeAngle: onAngle, onNodeViewImage: onViewImage,
        onNodeExtractVideoFrames: onExtractVideoFrames, onNodeReversePrompt: onReversePrompt, onNodeToggleFreeResize: onToggleFreeResize,
        onNodeToggleLocked: onToggleLocked, onNodeCopyPrompt: copyImagePrompt,
        onNodeSubtitles: onSubtitles, onNodeTimeline: onTimeline,
    } as Partial<ToolbarHandlers> as ToolbarHandlers;

    const nodeHoverCtx: ToolContext = {
        selectedCount: 0,
        selectedNodeTypes: new Set(),
        selectedVideoCount: 0,
        canvasTool: "move",
        workspaceMode: workspaceMode || "professional",
        isProjectLinked: false,
        canUndo: false,
        canRedo: false,
        node,
        nodeMetadata: node.metadata,
        extractingVideoFrame,
        mergingVideos: false,
        addPanelOpen: false,
        appearancePanelOpen: false,
        settingsPanelOpen: false,
        handlers: nodeHoverHandlers,
    };

    // 注册表只负责动作合同与适用性，Dock 的业务分组在此处唯一确定（对齐 OA）。
    const registryTools = resolveToolbarTools("node-hover", nodeHoverCtx, null);
    const otherRegistryTools = registryTools.filter((tool) => tool.id !== "node-lock");
    const otherTools: ToolbarTool[] = otherRegistryTools.map((tool) => ({
        id: tool.id,
        title: typeof tool.label === "function" ? tool.label(nodeHoverCtx) : tool.label,
        label: tool.displayLabel ? (typeof tool.displayLabel === "function" ? tool.displayLabel(nodeHoverCtx) : tool.displayLabel) : (typeof tool.label === "function" ? tool.label(nodeHoverCtx) : tool.label),
        icon: typeof tool.icon === "function" ? tool.icon(nodeHoverCtx) : tool.icon,
        active: tool.active?.(nodeHoverCtx),
        danger: tool.danger,
        disabled: tool.disabled?.(nodeHoverCtx),
        onClick: () => tool.run(nodeHoverCtx),
    }));
    const allTools: ToolbarTool[] = hasImage
        ? [...otherTools, ...imageTools.map((tool) => ({ id: tool.id, title: tool.title, label: tool.label, icon: tool.icon, active: tool.active, danger: undefined, disabled: undefined, onClick: tool.onClick }))]
        : otherTools;
    const selectableImageToolbarTools = allTools.filter((tool): tool is ToolbarTool & { id: ImageQuickToolId } => isImageQuickToolId(tool.id));
    const toolById = new Map(allTools.map((tool) => [tool.id, tool] as const));
    const takeTools = (ids: string[]) => ids.map((id) => toolById.get(id)).filter((tool): tool is ToolbarTool => Boolean(tool));
    const imageBaseTools = takeTools(hasImage ? ["delete", "download"] : ["delete", "uploadImage"]);
    const imageEditTools = takeTools(["maskEdit", "crop", "split"]);
    const imagePortraitTools = takeTools(["emotion", "portraitTexture"]).map((tool) => (tool.id === "emotion" ? { ...tool, label: canvasT("videoCanvas.nodeUi.emotionShort", "人物情绪") } : tool));
    const imageAngleTool = toolById.get("angle");
    const videoTools = takeTools(["delete", "download", "subtitles", "timeline", "extractFrames", "uploadVideo"]).map((tool) => {
        if (tool.id === "extractFrames") return { ...tool, label: canvasT("videoCanvas.toolbar.extractFrame", "画面") };
        return tool;
    });
    const genericTools = takeTools(isAudio ? ["delete", "download", "timeline", "uploadAudio"] : isEditableText ? ["delete", "edit", "editText", "generateImage", "saveAsset"] : ["delete", "info", "config"]);
    const visibleToolIds = new Set([
        ...(isImage ? [...imageBaseTools, ...imageEditTools, ...imagePortraitTools, ...(imageAngleTool ? [imageAngleTool] : [])] : isVideo ? videoTools : genericTools).map((tool) => tool.id),
    ]);
    const overflowTools = allTools
        .filter((tool) => !visibleToolIds.has(tool.id))
        .map((tool) => (tool.id === "edit" && (isImage || isVideo) ? { ...tool, label: canvasT("videoCanvas.nodeUi.genSettings", "生成设置") } : tool));
    // 专业模式图片：把「管理快捷工具」放入更多，保证入口与 OA「更多」一致可见
    if (hasImage) {
        overflowTools.push({
            id: "manage-image-quick-tools",
            title: canvasT("videoCanvas.nodeUi.manageQuickTools", "管理快捷工具"),
            label: canvasT("videoCanvas.nodeUi.manageQuickTools", "管理快捷工具"),
            icon: <Settings2 className="size-3.5" />,
            onClick: () => openImageToolSettings(),
        });
    }
    const lockTool: ToolbarTool = {
        id: "node-lock",
        title: node.metadata?.locked ? canvasT("videoCanvas.toolbar.unlockLong", "解锁节点") : canvasT("videoCanvas.toolbar.lockLong", "锁定位置和尺寸"),
        label: node.metadata?.locked ? canvasT("videoCanvas.toolbar.unlock", "解锁") : canvasT("videoCanvas.toolbar.lock", "锁定"),
        icon: node.metadata?.locked ? <Unlock className="size-3.5" /> : <Lock className="size-3.5" />,
        active: Boolean(node.metadata?.locked),
        onClick: () => onToggleLocked(node),
    };
    const handleMenuOpenChange = (menuId: string, open: boolean) => {
        setOpenMenuId((current) => (open ? menuId : current === menuId ? null : current));
        if (open) onKeep(node.id);
        else if (!imageToolSettingsOpenRef.current) onLeave();
    };

    const closeImageToolSettings = () => {
        imageToolSettingsOpenRef.current = false;
        setImageToolSettingsOpen(false);
        onLeave();
    };

    const setDraftImageToolVisible = (id: ImageQuickToolId, visible: boolean) => {
        setDraftImageToolIds((current) => {
            const selected = new Set(current);
            if (visible && selected.size >= MAX_IMAGE_QUICK_TOOLS) {
                message.warning(canvasT("videoCanvas.nodeUi.maxQuickTools", "最多固定 {{count}} 个快捷工具", { count: MAX_IMAGE_QUICK_TOOLS }));
                return current;
            }
            if (visible) selected.add(id);
            else selected.delete(id);
            return selectableImageToolbarTools.filter((tool) => selected.has(tool.id)).map((tool) => tool.id);
        });
    };

    const saveImageToolSettings = () => {
        setQuickImageToolIds(draftImageToolIds);
        setShowDockLabels(draftShowDockLabels);
        window.localStorage.setItem(IMAGE_QUICK_TOOLS_STORAGE_KEY, JSON.stringify(draftImageToolIds));
        window.localStorage.setItem(NODE_DOCK_LABELS_STORAGE_KEY, draftShowDockLabels ? "1" : "0");
        closeImageToolSettings();
    };

    const dockShellStyle = canvasDockStyle(theme, theme.node.text);
    const labeledDockStyle = {
        ...dockShellStyle,
        boxShadow: `0 8px 28px ${theme.spatial.shadow}`,
    };
    const moreLabel = canvasT("videoCanvas.nodeUi.more", "更多");

    return (
        <>
            <div
                ref={toolbarRef}
                className="canvas-node-toolbar absolute z-[var(--z-node-toolbar)] flex -translate-x-1/2 -translate-y-full items-end justify-center overflow-visible"
                style={{ left: anchor.left, top: anchor.top, width: "max-content", maxWidth: `min(calc(100% - 20px), ${showDockLabels ? 960 : 560}px)`, color: theme.node.text }}
                onMouseEnter={() => onKeep(node.id)}
                onMouseLeave={() => {
                    if (!imageToolSettingsOpenRef.current && !openMenuId) onLeave();
                }}
                onMouseDown={(event) => event.stopPropagation()}
                onPointerDown={(event) => event.stopPropagation()}
            >
                <div
                    role="toolbar"
                    aria-label={canvasT("videoCanvas.nodeUi.quickToolsAria", "节点快捷工具")}
                    className="aceternity-floating-dock thin-scrollbar relative flex h-9 max-w-full items-center gap-0.5 overflow-x-auto overflow-y-hidden rounded-[var(--r-lg)] border px-1.5 py-0.5 backdrop-blur-2xl"
                    style={labeledDockStyle}
                >
                    <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto overflow-y-hidden [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                        {isImage ? (
                            <>
                                {imageBaseTools.map((tool) => <NodeDockToolButton key={tool.id} tool={tool} showLabel={showDockLabels} />)}
                                {imageEditTools.length ? <NodeDockMenuButton menuId="image-edit" label={canvasT("videoCanvas.nodeUi.editGroup", "编辑")} icon={imageEditTools[0].icon} tools={imageEditTools} openMenuId={openMenuId} onOpenChange={handleMenuOpenChange} showLabel={showDockLabels} /> : null}
                                {imagePortraitTools.length ? <NodeDockMenuButton menuId="image-portrait" label={canvasT("videoCanvas.nodeUi.portraitGroup", "人物调整")} icon={imagePortraitTools[0].icon} tools={imagePortraitTools} openMenuId={openMenuId} onOpenChange={handleMenuOpenChange} showLabel={showDockLabels} /> : null}
                                {imageAngleTool ? <NodeDockToolButton tool={imageAngleTool} showLabel={showDockLabels} /> : null}
                            </>
                        ) : isVideo ? (
                            videoTools.map((tool) => <NodeDockToolButton key={tool.id} tool={tool} showLabel={showDockLabels} />)
                        ) : (
                            genericTools.map((tool) => <NodeDockToolButton key={tool.id} tool={tool} showLabel={showDockLabels} />)
                        )}
                    </div>
                    <span aria-hidden className="aceternity-dock-separator mx-1.5 h-6 w-px shrink-0" />
                    <div className="flex shrink-0 items-center gap-0.5">
                        <NodeDockToolButton tool={lockTool} showLabel={showDockLabels} />
                        {overflowTools.length ? (
                            <NodeDockMenuButton menuId="more" label={moreLabel} icon={<Ellipsis className="size-3.5" />} tools={overflowTools} openMenuId={openMenuId} onOpenChange={handleMenuOpenChange} showLabel={showDockLabels} placement="topRight" />
                        ) : null}
                    </div>
                </div>
            </div>
            {hasImage ? (
                <ImageToolSettingsModal
                    open={imageToolSettingsOpen}
                    tools={selectableImageToolbarTools}
                    selectedIds={draftImageToolIds}
                    showLabels={draftShowDockLabels}
                    onToggle={setDraftImageToolVisible}
                    onShowLabelsChange={setDraftShowDockLabels}
                    onCancel={closeImageToolSettings}
                    onSave={saveImageToolSettings}
                />
            ) : null}
        </>
    );
}


function NodeDockToolButton({ tool, showLabel = true }: { tool: ToolbarTool; showLabel?: boolean }) {
    return (
        <button
            type="button"
            className={`aceternity-dock-command is-quiet pointer-events-auto inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-[var(--dock-item-radius)] outline-none ${showLabel ? "is-labeled px-2.5" : "size-8"} ${tool.active ? "is-active" : ""} ${tool.danger ? "is-danger" : ""}`}
            aria-label={tool.title || tool.label}
            aria-pressed={tool.active || undefined}
            disabled={tool.disabled}
            title={tool.title || tool.label}
            onClick={tool.onClick}
        >
            <span className="grid size-3.5 shrink-0 place-items-center">{tool.icon}</span>
            {showLabel ? <span className="inline-flex h-4 items-center whitespace-nowrap text-[var(--fs-label)] font-medium leading-none">{tool.label}</span> : null}
        </button>
    );
}

function NodeDockMenuButton({
    menuId,
    label,
    icon,
    tools,
    openMenuId,
    onOpenChange,
    placement = "top",
    showLabel = true,
}: {
    menuId: string;
    label: string;
    icon: ReactNode;
    tools: ToolbarTool[];
    openMenuId: string | null;
    onOpenChange: (menuId: string, open: boolean) => void;
    placement?: "top" | "topRight";
    showLabel?: boolean;
}) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const triggerRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const open = openMenuId === menuId;
    const close = useCallback(() => onOpenChange(menuId, false), [menuId, onOpenChange]);
    const rect = useAnchoredOverlay(open, triggerRef, panelRef, close);
    const geometry = rect
        ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, {
            width: 220,
            placement: placement === "topRight" ? "topRight" : "top",
            estimatedHeight: 8 + tools.length * 28,
        })
        : null;

    return (
        <>
            <button
                ref={triggerRef}
                type="button"
                className={`aceternity-dock-command is-quiet pointer-events-auto inline-flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-[var(--dock-item-radius)] outline-none ${showLabel ? "is-labeled px-2.5" : "size-8"} ${open ? "is-active" : ""}`}
                aria-label={label}
                aria-expanded={open}
                title={label}
                onClick={() => onOpenChange(menuId, !open)}
            >
                <span className="grid size-3.5 shrink-0 place-items-center">{icon}</span>
                {showLabel ? (
                    <>
                        <span className="inline-flex h-4 items-center whitespace-nowrap text-[var(--fs-label)] font-medium leading-none">{label}</span>
                        <ChevronDown className="size-3 shrink-0 opacity-55" />
                    </>
                ) : null}
            </button>
            {open && geometry
                ? createPortal(
                    <div
                        ref={panelRef}
                        className="canvas-overlay"
                        style={{ ...overlayPanelStyle(theme, geometry), padding: 4 }}
                        onPointerDown={(event) => event.stopPropagation()}
                        onMouseDown={(event) => event.stopPropagation()}
                    >
                        {tools.map((tool) => (
                            <CanvasMenuRow
                                key={tool.id}
                                icon={tool.icon}
                                label={tool.label}
                                disabled={tool.disabled}
                                danger={tool.danger}
                                onClick={() => {
                                    tool.onClick();
                                    close();
                                }}
                            />
                        ))}
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}

export function CanvasNodeInfoModal({ node, open, onClose, onMetadataChange, readOnly = false, onUnauthorized }: { node: CanvasNodeData | null; open: boolean; onClose: () => void; onMetadataChange?: (nodeId: string, metadata: Partial<CanvasNodeMetadata>) => void; readOnly?: boolean; onUnauthorized?: () => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [view, setView] = useState<"info" | "json">("info");
    const [assetTags, setAssetTags] = useState<string[]>([]);
    const [assetTagInput, setAssetTagInput] = useState("");
    const [assetCategory, setAssetCategory] = useState<CanvasAssetCategory>("other");
    const imageBytes = node?.type === CanvasNodeType.Image && node.metadata?.content ? getDataUrlByteSize(node.metadata.content) : 0;
    const batchCount = node?.type === CanvasNodeType.Image ? node.metadata?.batchChildIds?.length || 0 : 0;
    const nodeTypeLabel = node ? getNodeLabel(node.type) : canvasT("videoCanvas.menu.fallbackNode", "节点");
    const json = useMemo(() => {
        if (!node) return "";
        return JSON.stringify(
            node,
            (key, value) => {
                if (key === "title") return undefined;
                if (key === "content" && typeof value === "string" && value.startsWith("data:image/")) {
                    return "[base64 image]";
                }
                return value;
            },
            2,
        );
    }, [node]);

    useEffect(() => {
        if (open) setView("info");
    }, [node?.id, open]);

    useEffect(() => {
        setAssetTags(node?.metadata?.assetTags || []);
        setAssetTagInput("");
        setAssetCategory(node ? canvasNodeAssetCategory(node) : "other");
    }, [node?.id, node?.metadata?.assetCategory, node?.metadata?.assetTags]);

    const saveAssetCategory = (category: CanvasAssetCategory) => {
        if (!node || node.type !== CanvasNodeType.Image) return;
        setAssetCategory(category);
        onMetadataChange?.(node.id, { assetCategory: category });
    };

    const saveAssetTags = (nextTags: string[]) => {
        if (!node || node.type !== CanvasNodeType.Image) return;
        const tags = Array.from(new Set(nextTags.map((item) => item.trim()).filter(Boolean)));
        setAssetTags(tags);
        onMetadataChange?.(node.id, { assetTags: tags });
    };

    const addAssetTag = () => {
        const tags = assetTagInput
            .split(/\n|,|，/)
            .map((item) => item.trim())
            .filter(Boolean);
        if (!tags.length) return;
        saveAssetTags([...assetTags, ...tags]);
        setAssetTagInput("");
    };

    const removeAssetTag = (tag: string) => {
        saveAssetTags(assetTags.filter((item) => item !== tag));
    };

    const title = (
        <div className="flex items-center justify-between gap-4 pr-10">
            <div className="min-w-0">
                <div className="text-[var(--fs-heading-lg)] font-semibold tracking-[-0.02em]">{canvasT("videoCanvas.nodeUi.infoTitle", "节点信息")}</div>
                {node ? <div className="mt-0.5 truncate text-xs opacity-45">{node.id}</div> : null}
            </div>
            <Segmented
                size="small"
                value={view}
                onChange={(value) => setView(value as "info" | "json")}
                options={[
                    { label: canvasT("videoCanvas.nodeUi.infoTab", "信息"), value: "info" },
                    { label: "JSON", value: "json" },
                ]}
            />
        </div>
    );

    return (
        <Modal
            className="canvas-node-info-modal"
            title={title}
            open={open && Boolean(node)}
            centered
            footer={null}
            width={720}
            onCancel={onClose}
            styles={{ body: { paddingTop: 8 } }}
        >
            {node ? (
                <div className="h-[min(68vh,640px)] min-h-[420px] text-sm" style={{ color: theme.node.text }}>
                    {view === "info" ? (
                        <div className="thin-scrollbar h-full space-y-4 overflow-auto pr-1">
                            <div className="grid gap-2 rounded-2xl border p-3" style={{ background: theme.node.fill, borderColor: theme.node.stroke }}>
                                <div className="grid grid-cols-2 gap-2 max-sm:grid-cols-1">
                                    <InfoRow label={canvasT("videoCanvas.nodeUi.infoType", "类型")} value={nodeTypeLabel} />
                                    <InfoRow label={canvasT("videoCanvas.nodeUi.infoStatus", "状态")} value={node.metadata?.status || "idle"} />
                                    <InfoRow label={canvasT("videoCanvas.nodeUi.infoSize", "尺寸")} value={`${Math.round(node.width)} x ${Math.round(node.height)}`} />
                                    <InfoRow label={canvasT("videoCanvas.nodeUi.infoPosition", "位置")} value={`${Math.round(node.position.x)}, ${Math.round(node.position.y)}`} />
                                    {batchCount > 1 ? <InfoRow label={canvasT("videoCanvas.nodeUi.infoImageGroup", "图片组")} value={canvasT("videoCanvas.nodeUi.infoImageCount", "{{count}} 张", { count: batchCount })} /> : null}
                                    {imageBytes ? <InfoRow label={canvasT("videoCanvas.nodeUi.infoImageBytes", "图片大小")} value={formatBytes(imageBytes)} /> : null}
                                </div>
                                {node.type === CanvasNodeType.Image ? (
                                    <div className="border-t pt-3" style={{ borderColor: theme.toolbar.border }}>
                                        <div className="mb-2 text-xs font-medium opacity-45">{canvasT("videoCanvas.nodeUi.assetCategory", "项目资产分类")}</div>
                                        <div className="flex flex-wrap gap-1.5">
                                            {assetCategoryOptions.map((option) => {
                                                const active = assetCategory === option.value;
                                                return <button key={option.value} type="button" disabled={readOnly} onClick={() => saveAssetCategory(option.value)} className="h-7 rounded-md border px-2 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-60" style={{ borderColor: active ? theme.accent.primary : theme.toolbar.border, background: active ? theme.accent.primarySoft : theme.toolbar.panel, color: active ? theme.accent.primary : theme.node.muted }}>{option.label}</button>;
                                            })}
                                        </div>
                                        <div className="mt-2 text-[var(--fs-label)] leading-5 opacity-45">{canvasT("videoCanvas.nodeUi.assetCategoryHint", "生成后会按此分类进入项目资产；角色、场景和画风工作流会自动预填。")}</div>
                                    </div>
                                ) : null}
                                {node.metadata?.prompt ? (
                                    <div className="rounded-xl border px-3 py-2" style={{ borderColor: theme.toolbar.border, background: theme.toolbar.panel }}>
                                        <div className="mb-1 text-xs font-medium opacity-45">{canvasT("videoCanvas.nodeUi.prompt", "提示词")}</div>
                                        <div className="whitespace-pre-wrap break-words leading-6">{node.metadata.prompt}</div>
                                    </div>
                                ) : null}
                                {node.type === CanvasNodeType.Skill && node.metadata?.skillSnapshot ? (
                                    <div className="rounded-xl border px-3 py-2" style={{ borderColor: theme.toolbar.border, background: theme.toolbar.panel }}>
                                        <div className="mb-1 text-xs font-medium opacity-45">{canvasT("videoCanvas.nodeUi.skillTemplate", "技能模板")}</div>
                                        <div className="whitespace-pre-wrap break-words leading-6">{node.metadata.skillSnapshot.template}</div>
                                        {node.metadata.skillSnapshot.outputContract ? (
                                            <>
                                                <div className="mb-1 mt-3 text-xs font-medium opacity-45">{canvasT("videoCanvas.nodeUi.outputContract", "输出约束")}</div>
                                                <div className="whitespace-pre-wrap break-words leading-6">{node.metadata.skillSnapshot.outputContract}</div>
                                            </>
                                        ) : null}
                                    </div>
                                ) : null}
                            </div>

                            {node.type === CanvasNodeType.Image ? (
                                <div className="rounded-2xl border p-3" style={{ background: theme.toolbar.panel, borderColor: theme.node.stroke }}>
                                    <div className="mb-2 flex items-center justify-between gap-3">
                                        <div>
                                            <div className="text-sm font-semibold">{canvasT("videoCanvas.nodeUi.assetTags", "资产标签")}</div>
                                            <div className="mt-0.5 text-xs opacity-45">{canvasT("videoCanvas.nodeUi.assetTagsHint", "一条标签描述一个角色、环境、道具或镜头用途。")}</div>
                                        </div>
                                        <span className="shrink-0 text-xs opacity-45">{canvasT("videoCanvas.nodeUi.tagCount", "{{count}} 条", { count: assetTags.length })}</span>
                                    </div>
                                    {readOnly ? (
                                        <div className="mb-2 rounded-lg border px-3 py-2 text-xs opacity-55" style={{ borderColor: theme.toolbar.border }}>
                                            {canvasT("videoCanvas.nodeUi.tagsReadOnly", "分享画布为只读，标签无法编辑。")}
                                        </div>
                                    ) : (
                                        <div className="flex gap-2">
                                            <Input
                                                value={assetTagInput}
                                                placeholder={canvasT("videoCanvas.nodeUi.tagPlaceholder", "例如：角色: 张三")}
                                                onChange={(event) => setAssetTagInput(event.target.value)}
                                                onPressEnter={addAssetTag}
                                            />
                                            <Button type="primary" icon={<Plus className="size-4" />} disabled={!assetTagInput.trim()} onClick={addAssetTag}>
                                                {canvasT("videoCanvas.nodeUi.tagAdd", "加入")}
                                            </Button>
                                        </div>
                                    )}
                                    <div className="mt-3 flex min-h-10 flex-wrap gap-2 rounded-xl border px-2 py-2" style={{ borderColor: theme.toolbar.border, background: theme.node.fill }}>
                                        {assetTags.length ? (
                                            assetTags.map((tag) => (
                                                <Tag key={tag} closable={!readOnly} onClose={() => (readOnly ? onUnauthorized?.() : removeAssetTag(tag))} className="!m-0 !rounded-lg !px-2 !py-1 !text-sm">
                                                    {tag}
                                                </Tag>
                                            ))
                                        ) : (
                                            <span className="px-1 py-1 text-xs opacity-40">{readOnly ? canvasT("videoCanvas.nodeUi.noTags", "暂无标签") : canvasT("videoCanvas.nodeUi.noTagsHint", "还没有标签，输入后点击“加入”或按 Enter。")}</span>
                                        )}
                                    </div>
                                </div>
                            ) : null}

                            {node.metadata?.errorDetails ? (
                                <div className="rounded-2xl border p-3 text-red-500" style={{ borderColor: theme.node.stroke }}>
                                    {formatCanvasUserError(node.metadata.errorDetails)}
                                </div>
                            ) : null}
                        </div>
                    ) : (
                        <pre className="thin-scrollbar h-full overflow-auto rounded-2xl border p-3 text-xs leading-5" style={{ background: theme.node.fill, borderColor: theme.node.stroke, color: theme.node.text }}>
                            {json}
                        </pre>
                    )}
                </div>
            ) : null}
        </Modal>
    );
}

function InfoRow({ label, value }: { label: string; value: ReactNode }) {
    return (
        <div className="rounded-xl border px-3 py-2" style={{ borderColor: "rgba(148,163,184,.22)" }}>
            <div className="mb-1 text-xs font-medium opacity-45">{label}</div>
            <div className="min-w-0 whitespace-pre-wrap break-words text-sm font-medium leading-5">{value}</div>
        </div>
    );
}
