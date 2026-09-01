import { AnimatePresence, useReducedMotion } from "motion/react";
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { ArrowLeft, Bookmark, Check, ChevronRight, Clipboard, Copy, FolderOpen, FolderPlus, Image as ImageIcon, Layers3, Link2, Maximize2, PanelTop, Pencil, Plus, Redo2, Tags, Trash2, Undo2, Upload, UserRound } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CanvasCreateMenu, type CanvasCreateCommand } from "@oc/components/canvas/canvas-create-menu";
import { aceternityMotion } from "@oc/lib/aceternity-motion";
import { SpotlightSurface } from "@oc/components/ui/aceternity/spotlight-surface";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { ASSET_CATEGORY_OPTIONS } from "@oc/lib/asset-category";
import { canvasNodeAssetCategory } from "@oc/lib/canvas/canvas-node-asset";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeLabel, getNodeListLabel } from "@oc/lib/canvas/node-registry";
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
    const reducedMotion = useReducedMotion();
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
            <SpotlightSurface
                spotlightColor={theme.toolbar.itemHover}
                data-canvas-context-menu
                initial={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.97, x: -3, y: -3 }}
                animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
                transition={{ duration: aceternityMotion.duration.instant, ease: aceternityMotion.easing.enter }}
                className="aceternity-floating-panel fixed z-[var(--z-popover)] flex w-[224px] max-h-[calc(100vh-56px)] origin-top-left flex-col overflow-hidden rounded-xl border p-1.5 backdrop-blur-2xl"
                style={{ left: position.left, top: position.top, background: theme.spatial.elevated, borderColor: theme.toolbar.border, color: theme.node.text, boxShadow: `0 30px 90px ${theme.spatial.shadow}` }}
                onContextMenu={(event) => event.preventDefault()}
                onPointerDown={(event) => event.stopPropagation()}
            >
                <div className="absolute inset-x-8 top-0 h-px" style={{ background: `linear-gradient(90deg, transparent, ${theme.toolbar.border}, transparent)` }} />
                <div className={`min-h-0 overflow-x-hidden ${menu.type === "canvas" && !categoryOpen ? "overflow-y-hidden" : "hover-scrollbar overflow-y-auto"}`}>
                    {menu.type === "node" && isMedia && categoryOpen ? (
                        <>
                            <MenuHeader title={canvasT("videoCanvas.menu.setAssetCategory", "设置资产分类")} description={node?.title || nodeTypeLabel(node)} onBack={() => setCategoryOpen(false)} />
                            <MenuSection label={canvasT("videoCanvas.menu.projectUse", "项目用途")} />
                            {assetCategoryOptions().map((option) => (
                                <MenuButton
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
                            <MenuHeader title={canvasT("videoCanvas.menu.canvasCommands", "画布命令")} />
                            <MenuButton icon={<Plus className="size-4" />} label={canvasT("videoCanvas.menu.addNode", "添加节点")} chevron active={addOpen} onClick={() => setAddOpen((value) => !value)} />
                            <MenuButton icon={<Upload className="size-4" />} label={canvasT("videoCanvas.menu.uploadHere", "上传到这里")} onClick={() => runAction(onUpload)} />
                            {!isProjectLinked ? <MenuButton icon={<FolderOpen className="size-4" />} label={canvasT("videoCanvas.menu.insertFromAssets", "从素材库插入")} onClick={() => runAction(onOpenAssets)} /> : null}
                            <MenuDivider />
                            <MenuSection label={canvasT("videoCanvas.menu.historyClipboard", "历史与剪贴板")} />
                            <MenuButton icon={<Undo2 className="size-4" />} label={canvasT("videoCanvas.menu.undo", "撤销")} shortcut="⌘Z" disabled={!canUndo} onClick={() => runAction(onUndo)} />
                            <MenuButton icon={<Redo2 className="size-4" />} label={canvasT("videoCanvas.menu.redo", "重做")} shortcut="⇧⌘Z" disabled={!canRedo} onClick={() => runAction(onRedo)} />
                            <MenuButton icon={<Clipboard className="size-4" />} label={canvasT("videoCanvas.menu.paste", "粘贴")} shortcut="⌘V" disabled={!canPaste} onClick={() => runAction(onPaste)} />
                        </>
                    ) : menu.type === "node" ? (
                        <>
                            {isCharacterReference ? (
                                <>
                                    <MenuHeader title={canvasT("videoCanvas.menu.characterCard", "角色卡")} description={node?.metadata?.characterName || node?.title} />
                                    <MenuSection label={canvasT("videoCanvas.menu.characterRef", "角色引用")} />
                                    <MenuButton icon={<UserRound />} label={canvasT("videoCanvas.menu.viewCharacter", "查看角色详情")} onClick={() => runAction(onEditText)} />
                                    <MenuDivider />
                                    <MenuSection label={canvasT("videoCanvas.menu.node", "节点")} />
                                    <MenuButton icon={<Copy />} label={canvasT("videoCanvas.menu.copyCharacterRef", "复制角色引用")} shortcut="⌘C" onClick={() => runAction(onCopyNode)} />
                                    <MenuButton icon={<Layers3 />} label={canvasT("videoCanvas.menu.createRefCopy", "创建引用副本")} shortcut="⌘D" onClick={() => runAction(onDuplicate)} />
                                    <MenuButton icon={<Trash2 />} label={canvasT("videoCanvas.menu.deleteNode", "删除节点")} danger onClick={() => runAction(onDelete)} />
                                </>
                            ) : isMedia ? (
                                <>
                                    <MenuHeader title={isImage ? getNodeLabel(CanvasNodeType.Image) : getNodeLabel(CanvasNodeType.Video)} description={node?.title || nodeTypeLabel(node)} />
                                    <MenuSection label={canvasT("videoCanvas.menu.viewArchive", "查看与归档")} />
                                    <MenuButton icon={<Maximize2 />} label={canvasT("videoCanvas.menu.fullscreenPreview", "进入全景预览")} disabled={!canOpenPreview} onClick={() => runAction(onViewMedia)} />
                                    <MenuButton icon={<Tags />} label={canvasT("videoCanvas.menu.setAssetCategory", "设置资产分类")} chevron onClick={() => setCategoryOpen(true)} />
                                    {isImage ? <MenuButton icon={isTvCover ? <Check /> : <Bookmark />} label={isTvCover ? canvasT("videoCanvas.menu.currentCover", "当前封面") : canvasT("videoCanvas.menu.setAsCover", "设为封面")} active={isTvCover} disabled={!canSetCover && !isTvCover} onClick={() => runAction(() => onSetTvCover(!isTvCover))} /> : null}
                                    <MenuDivider />
                                    <MenuSection label={canvasT("videoCanvas.menu.node", "节点")} />
                                    <MenuButton icon={<Copy />} label={canvasT("videoCanvas.menu.copyNode", "复制节点")} shortcut="⌘C" onClick={() => runAction(onCopyNode)} />
                                    <MenuButton icon={<Link2 />} label={isImage ? canvasT("videoCanvas.menu.copyImageUrl", "复制图片地址") : canvasT("videoCanvas.menu.copyVideoUrl", "复制视频地址")} disabled={!canCopyMediaUrl} onClick={() => runAction(onCopyMediaUrl)} />
                                    <MenuButton icon={<Layers3 />} label={canvasT("videoCanvas.menu.createVariant", "创建参数变体")} shortcut="⌘D" onClick={() => runAction(onDuplicate)} />
                                    <MenuButton icon={<Trash2 />} label={canvasT("videoCanvas.menu.deleteNode", "删除节点")} danger onClick={() => runAction(onDelete)} />
                                </>
                            ) : (
                                <>
                                    <MenuHeader title={node?.title || nodeTypeLabel(node)} />
                                    <MenuSection label={canvasT("videoCanvas.menu.nodeActions", "节点操作")} />
                                    {isFrame ? <MenuButton icon={<PanelTop />} label={node?.metadata?.frame?.collapsed ? canvasT("videoCanvas.menu.expandFrame", "展开背板") : canvasT("videoCanvas.menu.collapseFrame", "折叠背板")} onClick={() => runAction(onToggleFrame)} /> : <MenuButton icon={<FolderPlus />} label={canvasT("videoCanvas.menu.saveToAssets", "保存到我的素材")} disabled={!canSaveAsset} onClick={() => runAction(onSaveAsset)} />}
                                    {isText ? <MenuButton icon={<Maximize2 />} label={canvasT("videoCanvas.menu.expandEdit", "放大编辑")} onClick={() => runAction(onEditText)} /> : null}
                                    {isDrawing ? <MenuButton icon={<Pencil />} label={canvasT("videoCanvas.menu.openDrawing", "打开绘图")} onClick={() => runAction(onOpenDrawing)} /> : null}
                                    {isText ? <MenuButton icon={<ImageIcon />} label={canvasT("videoCanvas.menu.genImageFromText", "用文本生图")} disabled={!canGenerateFromText} onClick={() => runAction(onGenerateImage)} /> : null}
                                    <MenuDivider />
                                    <MenuSection label={canvasT("videoCanvas.menu.copyAndContent", "副本与内容")} />
                                    <MenuButton icon={<Copy />} label={isFrame ? canvasT("videoCanvas.menu.copyFrameAndContent", "复制背板及内容") : canvasT("videoCanvas.menu.copyNode", "复制节点")} shortcut="⌘C" onClick={() => runAction(onCopyNode)} />
                                    {isText ? <MenuButton icon={<Clipboard />} label={canvasT("videoCanvas.menu.copyText", "复制文本")} disabled={!hasNodeContent} onClick={() => runAction(onCopyContent)} /> : null}
                                    <MenuButton icon={<Copy />} label={isFrame ? canvasT("videoCanvas.menu.createFrameCopy", "创建背板副本") : canvasT("videoCanvas.menu.createVariant", "创建参数变体")} shortcut="⌘D" onClick={() => runAction(onDuplicate)} />
                                    <MenuButton icon={<Clipboard />} label={canvasT("videoCanvas.menu.paste", "粘贴")} shortcut="⌘V" disabled={!canPaste} onClick={() => runAction(onPaste)} />
                                    <MenuButton icon={<Trash2 />} label={isFrame ? canvasT("videoCanvas.menu.deleteFrame", "删除背板") : canvasT("videoCanvas.menu.deleteNode", "删除节点")} danger onClick={() => runAction(onDelete)} />
                                </>
                            )}
                        </>
                    ) : (
                        <>
                            <MenuHeader title={canvasT("videoCanvas.menu.connection", "连接")} />
                            <MenuButton icon={<Trash2 className="size-4" />} label={canvasT("videoCanvas.menu.deleteConnection", "删除连接")} danger onClick={() => runAction(onDelete)} />
                        </>
                    )}
                </div>
            </SpotlightSurface>

            <AnimatePresence>
                {menu.type === "canvas" && addOpen ? (
                    <AddNodeContextMenu
                        parentPosition={position}
                        workspaceMode={workspaceMode}
                        isProjectLinked={isProjectLinked}
                        reducedMotion={Boolean(reducedMotion)}
                        onAddNode={(type) => runAction(() => onAddNode(type))}
                        onAddFolder={() => runAction(onAddFolder)}
                        onChooseStyle={() => runAction(onChooseStyle)}
                        onOpenDirector={() => runAction(() => onOpenDirector(menu.position))}
                        onUpload={() => runAction(onUpload)}
                        onOpenAssets={() => runAction(onOpenAssets)}
                        onOpenProjectCharacters={() => runAction(onOpenProjectCharacters)}
                    />
                ) : null}
            </AnimatePresence>
        </>
    );
}

function AddNodeContextMenu({ parentPosition, workspaceMode, isProjectLinked, reducedMotion, onAddNode, onAddFolder, onChooseStyle, onOpenDirector, onUpload, onOpenAssets, onOpenProjectCharacters }: { parentPosition: { left: number; top: number }; workspaceMode: CanvasWorkspaceMode; isProjectLinked: boolean; reducedMotion: boolean; onAddNode: (type: CanvasNodeType) => void; onAddFolder: () => void; onChooseStyle: () => void; onOpenDirector: () => void; onUpload: () => void; onOpenAssets: () => void; onOpenProjectCharacters: () => void }) {
    useTranslation();
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
        <SpotlightSurface
            spotlightColor={theme.toolbar.itemHover}
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, x: left > parentPosition.left ? -5 : 5, scale: 0.97 }}
            animate={{ opacity: 1, x: 0, scale: 1 }}
            exit={reducedMotion ? { opacity: 0 } : { opacity: 0, x: left > parentPosition.left ? -4 : 4, scale: 0.98 }}
            transition={{ duration: aceternityMotion.duration.instant, ease: aceternityMotion.easing.enter }}
            className="aceternity-floating-panel fixed z-[var(--z-popover)] w-[260px] origin-top overflow-hidden rounded-[var(--dock-radius)] border p-2 backdrop-blur-2xl"
            style={{ left, top: parentPosition.top, background: theme.spatial.elevated, borderColor: theme.toolbar.border, color: theme.node.text, boxShadow: `0 30px 90px ${theme.spatial.shadow}` }}
            onContextMenu={(event) => event.preventDefault()}
            onPointerDown={(event) => event.stopPropagation()}
        >
            <div className="absolute inset-x-8 top-0 h-px" style={{ background: `linear-gradient(90deg, transparent, ${theme.toolbar.border}, transparent)` }} />
            <CanvasCreateMenu commands={commands} />
        </SpotlightSurface>
    );
}

function MenuHeader({ title, description, onBack }: { title: string; description?: string; onBack?: () => void }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    return (
        <div className="mb-0.5 flex items-start gap-1 px-1.5 py-1.5">
            {onBack ? <button type="button" onClick={onBack} className="mt-0.5 grid size-6 shrink-0 place-items-center rounded-md outline-none hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/8" aria-label={canvasT("videoCanvas.menu.backMedia", "返回媒体操作")}><ArrowLeft className="size-3.5" /></button> : null}
            <span className="min-w-0"><span className="block truncate text-xs font-semibold">{title}</span>{description && description !== title ? <span className="mt-0.5 block truncate text-[var(--fs-micro)]" style={{ color: theme.node.muted }}>{description}</span> : null}</span>
        </div>
    );
}

function MenuSection({ label }: { label: string }) {
    return <div className="px-2 pb-1 pt-1.5 text-[var(--fs-micro)] font-medium opacity-45">{label}</div>;
}

function MenuButton({ icon, label, detail, shortcut, badge, chevron = false, active = false, disabled = false, danger = false, onClick }: { icon: ReactNode; label: string; detail?: string; shortcut?: string; badge?: string; chevron?: boolean; active?: boolean; disabled?: boolean; danger?: boolean; onClick?: () => void }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const color = danger ? theme.accent.danger : theme.node.text;
    return (
        <button
            type="button"
            className="canvas-menu-item group flex min-h-9 w-full min-w-0 items-center gap-2 rounded-lg border border-transparent px-1.5 py-1 text-left outline-none enabled:hover:border-black/10 enabled:hover:bg-black/5 focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-35 dark:enabled:hover:border-white/10 dark:enabled:hover:bg-white/8"
            style={{ color, background: active ? theme.toolbar.activeBg : undefined, "--tw-ring-color": theme.node.muted } as CSSProperties}
            disabled={disabled}
            onClick={onClick}
        >
            <span className="canvas-menu-item-icon grid size-7 shrink-0 place-items-center rounded-md border opacity-75 group-hover:opacity-100 [&_svg]:size-3.5" style={{ background: danger ? `${theme.accent.danger}12` : theme.spatial.surface, borderColor: danger ? `${theme.accent.danger}33` : theme.toolbar.border, color: danger ? theme.accent.danger : theme.node.text }}>{icon}</span>
            <span className="min-w-0 flex-1"><span className="flex items-center gap-1 text-xs font-medium"><span className="truncate">{label}</span>{badge ? <span className="rounded-full border px-1 py-0.5 text-[var(--fs-nano)] font-bold" style={{ background: theme.toolbar.activeBg, borderColor: theme.toolbar.border, color: theme.node.muted }}>{badge}</span> : null}</span>{detail ? <span className="mt-0.5 block truncate text-[var(--fs-micro)]" style={{ color: theme.node.muted }}>{detail}</span> : null}</span>
            {shortcut ? <span className="shrink-0 text-[var(--fs-micro)] opacity-38">{shortcut}</span> : null}
            {chevron ? <ChevronRight className="size-3 shrink-0 opacity-45 transition-transform group-hover:translate-x-0.5" /> : null}
        </button>
    );
}

function MenuDivider() {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    return <div className="mx-1.5 my-1 h-px" style={{ background: `linear-gradient(90deg, transparent, ${theme.toolbar.border}, transparent)` }} />;
}

function getContextMenuPosition(menu: ContextMenuState) {
    if (typeof window === "undefined") return { left: menu.x, top: menu.y };
    const width = 224;
    const estimatedHeight = menu.type === "node" ? Math.min(360, window.innerHeight - 72) : menu.type === "canvas" ? 250 : 84;
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

function nodeTypeLabel(node?: CanvasNodeData | null) {
    if (!node) return canvasT("videoCanvas.menu.fallbackNode", "节点");
    return getNodeListLabel(node.type);
}
