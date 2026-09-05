import { useEffect, useState } from "react";
import { ArrowLeft, Bookmark, Check, Clipboard, Copy, FolderOpen, FolderPlus, Image as ImageIcon, Layers3, Link2, Maximize2, PanelTop, Pencil, Plus, Redo2, Tags, Trash2, Undo2, Upload, UserRound } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CanvasCreateMenu, type CanvasCreateCommand } from "@oc/components/canvas/canvas-create-menu";
import { CanvasMenuRow, CanvasMenuSeparator, CanvasOverlay } from "@oc/components/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { ASSET_CATEGORY_OPTIONS } from "@oc/lib/asset-category";
import { canvasNodeAssetCategory } from "@oc/lib/canvas/canvas-node-asset";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { resolveAddNodeMenuCommands, type AddNodeMenuContext } from "@oc/lib/canvas/tool-registry";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasNodeType, type CanvasNodeData, type CanvasWorkspaceMode, type ContextMenuState, type Position } from "@oc/types/canvas";

type CanvasAssetCategory = NonNullable<NonNullable<CanvasNodeData["metadata"]>["assetCategory"]>;

function assetCategoryOptions(): Array<{ value: CanvasAssetCategory; label: string }> {
    return ASSET_CATEGORY_OPTIONS.map((option) => ({ value: option.value, label: option.label }));
}

type CanvasNodeContextMenuProps = {
    menu: ContextMenuState;
    node?: CanvasNodeData | null;
    workspaceMode?: CanvasWorkspaceMode;
    isProjectLinked?: boolean;
    canUndo: boolean;
    canRedo: boolean;
    canPaste: boolean;
    onClose: () => void;
    onAddNode: (type: CanvasNodeType) => void;
    onAddFolder: () => void;
    onChooseStyle: () => void;
    onOpenDirector: (position: Position) => void;
    onUpload: () => void;
    onOpenAssets: () => void;
    onOpenProjectCharacters: () => void;
    onUndo: () => void;
    onRedo: () => void;
    onPaste: () => void;
    onCopyNode: () => void;
    onDuplicate: () => void;
    onCreateGenerationCopy: () => void;
    onDelete: () => void;
    onSaveAsset: () => void;
    onViewMedia: () => void;
    onEditText: () => void;
    onOpenDrawing: () => void;
    onGenerateImage: () => void;
    onCopyContent: () => void;
    onCopyMediaUrl: () => void;
    onSetAssetCategory: (category: CanvasAssetCategory) => void;
    onSetTvCover: (enabled: boolean) => void;
    onToggleFrame: () => void;
};

export function CanvasNodeContextMenu({
    menu,
    node,
    workspaceMode = "professional",
    isProjectLinked = false,
    canUndo,
    canRedo,
    canPaste,
    onClose,
    onAddNode,
    onAddFolder,
    onChooseStyle,
    onOpenDirector,
    onUpload,
    onOpenAssets,
    onOpenProjectCharacters,
    onUndo,
    onRedo,
    onPaste,
    onCopyNode,
    onDuplicate,
    onCreateGenerationCopy,
    onDelete,
    onSaveAsset,
    onViewMedia,
    onEditText,
    onOpenDrawing,
    onGenerateImage,
    onCopyContent,
    onCopyMediaUrl,
    onSetAssetCategory,
    onSetTvCover,
    onToggleFrame,
}: CanvasNodeContextMenuProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [addOpen, setAddOpen] = useState(false);
    const [categoryOpen, setCategoryOpen] = useState(false);

    useEffect(() => {
        const close = (event: PointerEvent) => {
            const target = event.target;
            if (target instanceof Element && target.closest(".ant-popover")) return;
            onClose();
        };
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key !== "Escape") return;
            if (categoryOpen) setCategoryOpen(false);
            else onClose();
        };
        window.addEventListener("pointerdown", close);
        window.addEventListener("keydown", closeOnEscape);
        return () => {
            window.removeEventListener("pointerdown", close);
            window.removeEventListener("keydown", closeOnEscape);
        };
    }, [categoryOpen, onClose]);

    useEffect(() => {
        setAddOpen(false);
        setCategoryOpen(false);
    }, [menu.type, menu.x, menu.y]);

    const runAction = (action: () => void) => {
        action();
        onClose();
    };
    const nodeContent = typeof node?.metadata?.content === "string" ? node.metadata.content : "";
    const isImage = node?.type === CanvasNodeType.Image;
    const isText = node?.type === CanvasNodeType.Text;
    const isCharacterReference = Boolean(isText && node?.metadata?.workflowKind === "character" && node.metadata.characterAssetId);
    const isDrawing = node?.type === CanvasNodeType.Drawing;
    const isVideo = node?.type === CanvasNodeType.Video;
    const isMedia = isImage || isVideo;
    const isAudio = node?.type === CanvasNodeType.Audio;
    const isFrame = node?.type === CanvasNodeType.Frame;
    const hasNodeContent = isText ? Boolean(nodeContent.trim()) : Boolean(nodeContent);
    const canSaveAsset = Boolean(node && !isCharacterReference && (isText ? hasNodeContent : hasNodeContent && (isImage || isVideo || isAudio)));
    const canOpenPreview = Boolean(isMedia && hasNodeContent);
    const canGenerateFromText = Boolean(isText && !isCharacterReference && hasNodeContent);
    const canCopyMediaUrl = Boolean(isMedia && hasNodeContent);
    const isTvCover = Boolean(isImage && node?.metadata?.tvCover);
    const canSetCover = Boolean(isImage && (hasNodeContent || node?.metadata?.storageKey || node?.metadata?.mediaId));
    const assetCategory = node ? canvasNodeAssetCategory(node) : "other";
    const position = getContextMenuPosition(menu);

    return (
        <>
            <CanvasOverlay
                theme={theme}
                data-canvas-context-menu
                className="fixed z-[var(--z-popover)] flex w-[220px] max-h-[calc(100vh-56px)] origin-top-left flex-col overflow-hidden p-1"
                style={{ left: position.left, top: position.top }}
                onContextMenu={(event) => event.preventDefault()}
                onPointerDown={(event) => event.stopPropagation()}
            >
                <div className={`min-h-0 overflow-x-hidden ${menu.type === "canvas" && !categoryOpen ? "overflow-y-hidden" : "hover-scrollbar overflow-y-auto"}`}>
                    {menu.type === "node" && isMedia && categoryOpen ? (
                        <>
                            <CanvasMenuRow icon={<ArrowLeft />} label={canvasT("videoCanvas.menu.backMedia", "返回媒体操作")} onClick={() => setCategoryOpen(false)} />
                            <CanvasMenuSeparator />
                            {assetCategoryOptions().map((option) => (
                                <CanvasMenuRow
                                    key={option.value}
                                    icon={assetCategory === option.value ? <Check /> : <Tags />}
                                    label={option.label}
                                    active={assetCategory === option.value}
                                    onClick={() => runAction(() => onSetAssetCategory(option.value))}
                                />
                            ))}
                        </>
                    ) : menu.type === "canvas" ? (
                        <>
                            <CanvasMenuRow icon={<Plus />} label={canvasT("videoCanvas.menu.addNode", "添加节点")} chevron active={addOpen} onClick={() => setAddOpen((value) => !value)} />
                            <CanvasMenuRow icon={<Upload />} label={canvasT("videoCanvas.menu.uploadHere", "上传到这里")} onClick={() => runAction(onUpload)} />
                            <CanvasMenuRow icon={<FolderOpen />} label={canvasT("videoCanvas.menu.insertFromAssets", "从素材空间插入")} onClick={() => runAction(onOpenAssets)} />
                            <CanvasMenuSeparator />
                            <CanvasMenuRow icon={<Undo2 />} label={canvasT("videoCanvas.menu.undo", "撤销")} shortcut="⌘Z" disabled={!canUndo} onClick={() => runAction(onUndo)} />
                            <CanvasMenuRow icon={<Redo2 />} label={canvasT("videoCanvas.menu.redo", "重做")} shortcut="⇧⌘Z" disabled={!canRedo} onClick={() => runAction(onRedo)} />
                            <CanvasMenuRow icon={<Clipboard />} label={canvasT("videoCanvas.menu.paste", "粘贴")} shortcut="⌘V" disabled={!canPaste} onClick={() => runAction(onPaste)} />
                        </>
                    ) : menu.type === "node" ? (
                        <>
                            {isCharacterReference ? (
                                <>
                                    <CanvasMenuRow icon={<UserRound />} label={canvasT("videoCanvas.menu.viewCharacter", "查看角色详情")} onClick={() => runAction(onEditText)} />
                                    <CanvasMenuSeparator />
                                    <CanvasMenuRow icon={<Copy />} label={canvasT("videoCanvas.menu.copyCharacterRef", "复制角色引用")} shortcut="⌘C" onClick={() => runAction(onCopyNode)} />
                                    <CanvasMenuRow icon={<Layers3 />} label={canvasT("videoCanvas.menu.createRefCopy", "创建引用副本")} shortcut="⌘D" onClick={() => runAction(onDuplicate)} />
                                    <CanvasMenuRow icon={<Trash2 />} label={canvasT("videoCanvas.menu.deleteNode", "删除节点")} danger onClick={() => runAction(onDelete)} />
                                </>
                            ) : isMedia ? (
                                <>
                                    <CanvasMenuRow icon={<Maximize2 />} label={canvasT("videoCanvas.menu.fullscreenPreview", "进入全景预览")} disabled={!canOpenPreview} onClick={() => runAction(onViewMedia)} />
                                    <CanvasMenuRow icon={<Tags />} label={canvasT("videoCanvas.menu.setAssetCategory", "设置资产分类")} chevron onClick={() => setCategoryOpen(true)} />
                                    {isImage ? <CanvasMenuRow icon={isTvCover ? <Check /> : <Bookmark />} label={isTvCover ? canvasT("videoCanvas.menu.currentCover", "当前封面") : canvasT("videoCanvas.menu.setAsCover", "设为封面")} active={isTvCover} disabled={!canSetCover && !isTvCover} onClick={() => runAction(() => onSetTvCover(!isTvCover))} /> : null}
                                    <CanvasMenuSeparator />
                                    <CanvasMenuRow icon={<Copy />} label={canvasT("videoCanvas.menu.copyNode", "复制节点")} shortcut="⌘C" onClick={() => runAction(onCopyNode)} />
                                    <CanvasMenuRow icon={<Link2 />} label={isImage ? canvasT("videoCanvas.menu.copyImageUrl", "复制图片地址") : canvasT("videoCanvas.menu.copyVideoUrl", "复制视频地址")} disabled={!canCopyMediaUrl} onClick={() => runAction(onCopyMediaUrl)} />
                                    <CanvasMenuRow icon={<Copy />} label={canvasT("videoCanvas.menu.createGenerationCopy", "创建生成副本")} onClick={() => runAction(onCreateGenerationCopy)} />
                                    <CanvasMenuRow icon={<Layers3 />} label={canvasT("videoCanvas.menu.createVariant", "创建参数变体")} shortcut="⌘D" onClick={() => runAction(onDuplicate)} />
                                    <CanvasMenuRow icon={<Trash2 />} label={canvasT("videoCanvas.menu.deleteNode", "删除节点")} danger onClick={() => runAction(onDelete)} />
                                </>
                            ) : (
                                <>
                                    {isFrame ? <CanvasMenuRow icon={<PanelTop />} label={node?.metadata?.frame?.collapsed ? canvasT("videoCanvas.menu.expandFrame", "展开背板") : canvasT("videoCanvas.menu.collapseFrame", "折叠背板")} onClick={() => runAction(onToggleFrame)} /> : <CanvasMenuRow icon={<FolderPlus />} label={canvasT("videoCanvas.menu.saveToAssets", "保存到我的素材")} disabled={!canSaveAsset} onClick={() => runAction(onSaveAsset)} />}
                                    {isText ? <CanvasMenuRow icon={<Maximize2 />} label={canvasT("videoCanvas.menu.expandEdit", "放大编辑")} onClick={() => runAction(onEditText)} /> : null}
                                    {isDrawing ? <CanvasMenuRow icon={<Pencil />} label={canvasT("videoCanvas.menu.openDrawing", "打开绘图")} onClick={() => runAction(onOpenDrawing)} /> : null}
                                    {isText ? <CanvasMenuRow icon={<ImageIcon />} label={canvasT("videoCanvas.menu.genImageFromText", "用文本生图")} disabled={!canGenerateFromText} onClick={() => runAction(onGenerateImage)} /> : null}
                                    <CanvasMenuSeparator />
                                    <CanvasMenuRow icon={<Copy />} label={isFrame ? canvasT("videoCanvas.menu.copyFrameAndContent", "复制背板及内容") : canvasT("videoCanvas.menu.copyNode", "复制节点")} shortcut="⌘C" onClick={() => runAction(onCopyNode)} />
                                    {isText ? <CanvasMenuRow icon={<Clipboard />} label={canvasT("videoCanvas.menu.copyText", "复制文本")} disabled={!hasNodeContent} onClick={() => runAction(onCopyContent)} /> : null}
                                    <CanvasMenuRow icon={<Copy />} label={isFrame ? canvasT("videoCanvas.menu.createFrameCopy", "创建背板副本") : canvasT("videoCanvas.menu.createVariant", "创建参数变体")} shortcut="⌘D" onClick={() => runAction(onDuplicate)} />
                                    <CanvasMenuRow icon={<Clipboard />} label={canvasT("videoCanvas.menu.paste", "粘贴")} shortcut="⌘V" disabled={!canPaste} onClick={() => runAction(onPaste)} />
                                    <CanvasMenuRow icon={<Trash2 />} label={isFrame ? canvasT("videoCanvas.menu.deleteFrame", "删除背板") : canvasT("videoCanvas.menu.deleteNode", "删除节点")} danger onClick={() => runAction(onDelete)} />
                                </>
                            )}
                        </>
                    ) : (
                        <CanvasMenuRow icon={<Trash2 />} label={canvasT("videoCanvas.menu.deleteConnection", "删除连接")} danger onClick={() => runAction(onDelete)} />
                    )}
                </div>
            </CanvasOverlay>

            {menu.type === "canvas" && addOpen ? (
                <AddNodeContextMenu
                    parentPosition={position}
                    workspaceMode={workspaceMode}
                    isProjectLinked={isProjectLinked}
                    onAddNode={(type) => runAction(() => onAddNode(type))}
                    onAddFolder={() => runAction(onAddFolder)}
                    onChooseStyle={() => runAction(onChooseStyle)}
                    onOpenDirector={() => runAction(() => onOpenDirector(menu.position))}
                    onUpload={() => runAction(onUpload)}
                    onOpenAssets={() => runAction(onOpenAssets)}
                    onOpenProjectCharacters={() => runAction(onOpenProjectCharacters)}
                />
            ) : null}
        </>
    );
}

function AddNodeContextMenu({ parentPosition, workspaceMode, isProjectLinked, onAddNode, onAddFolder, onChooseStyle, onOpenDirector, onUpload, onOpenAssets, onOpenProjectCharacters }: { parentPosition: { left: number; top: number }; workspaceMode: CanvasWorkspaceMode; isProjectLinked: boolean; onAddNode: (type: CanvasNodeType) => void; onAddFolder: () => void; onChooseStyle: () => void; onOpenDirector: () => void; onUpload: () => void; onOpenAssets: () => void; onOpenProjectCharacters: () => void }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const left = getSubmenuLeft(parentPosition.left);
    const createContext: AddNodeMenuContext = {
        workspaceMode,
        isProjectLinked,
        handlers: {
            onAddText: () => onAddNode(CanvasNodeType.Text),
            onAddImage: () => onAddNode(CanvasNodeType.Image),
            onAddVideo: () => onAddNode(CanvasNodeType.Video),
            onAddAudio: () => onAddNode(CanvasNodeType.Audio),
            onAddScript: () => onAddNode(CanvasNodeType.Script),
            onAddFrame: () => onAddNode(CanvasNodeType.Frame),
            onAddFolder,
            onAddDrawing: () => onAddNode(CanvasNodeType.Drawing),
            onChooseStyle,
            onOpenDirector,
            onAddExtensionNode: (type) => onAddNode(type),
            onUpload,
            onOpenMyAssets: onOpenAssets,
            onOpenProjectCharacters,
        },
    };
    const commands: CanvasCreateCommand[] = resolveAddNodeMenuCommands(createContext).map((command) => ({
        id: command.id,
        label: command.label,
        icon: command.icon,
        badge: command.badge,
        section: command.section,
        onClick: () => command.run(createContext),
    }));

    return (
        <CanvasOverlay
            theme={theme}
            className="fixed z-[var(--z-popover)] w-[260px] origin-top overflow-hidden p-2"
            style={{ left, top: parentPosition.top }}
            onContextMenu={(event) => event.preventDefault()}
            onPointerDown={(event) => event.stopPropagation()}
        >
            <CanvasCreateMenu commands={commands} />
        </CanvasOverlay>
    );
}

function getContextMenuPosition(menu: ContextMenuState) {
    if (typeof window === "undefined") return { left: menu.x, top: menu.y };
    const width = 220;
    const estimatedHeight = menu.type === "node" ? Math.min(320, window.innerHeight - 72) : menu.type === "canvas" ? 220 : 48;
    return {
        left: clamp(menu.x, 12, Math.max(12, window.innerWidth - width - 12)),
        top: clamp(menu.y, 68, Math.max(68, window.innerHeight - estimatedHeight - 12)),
    };
}

function getSubmenuLeft(parentLeft: number) {
    if (typeof window === "undefined") return parentLeft + 192;
    return parentLeft + 224 + 8 + 260 <= window.innerWidth - 12 ? parentLeft + 232 : Math.max(12, parentLeft - 268);
}

function clamp(value: number, min: number, max: number) {
    return Math.min(Math.max(value, min), max);
}
