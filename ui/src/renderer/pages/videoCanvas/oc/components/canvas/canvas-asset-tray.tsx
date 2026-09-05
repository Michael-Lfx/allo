import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Box, Clapperboard, Crosshair, FolderOpen, ImageIcon, Images, MapPinned, Megaphone, Music2, Plus, Search, Sparkles, UserRound, Video, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CanvasChromeButton, CanvasOverlay } from "@oc/components/canvas/canvas-overlay";
import { aceternityMotion } from "@oc/lib/aceternity-motion";
import { canvasNodeDisplayUrl } from "@oc/lib/canvas/canvas-media-id";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { ASSET_SPACE_DRAMA_CATEGORIES, ASSET_SPACE_SOURCES, type AssetSpaceDramaCategory, type AssetSpaceItem, type AssetSpaceKind, type AssetSpaceSource } from "@oc/lib/canvas/canvas-asset-space";
import { useBriefingAssetPreview, useVimaxAssetPreview } from "@oc/lib/canvas/canvas-asset-space-media";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { AudioAsset, ImageAsset, VideoAsset } from "@oc/stores/use-asset-store";
import type { CanvasNodeData } from "@oc/types/canvas";
import { useAssetSpaceDramaCategoryCounts, useAssetSpaceKindCounts, useCanvasAssetSpace, useFilteredAssetSpace } from "@oc/pages/canvas/use-canvas-asset-space";

export const CANVAS_IMAGE_ASSET_DND_TYPE = "application/x-infinite-canvas-image-asset";
export const CANVAS_MEDIA_ASSET_DND_TYPE = "application/x-infinite-canvas-media-asset";

export type CanvasTrayMediaAsset = ImageAsset | VideoAsset | AudioAsset;

const GRID_COLUMNS = 2;
const GRID_ROW_ESTIMATE = 168;

type CanvasAssetTrayProps = {
    nodes: CanvasNodeData[];
    activeNodeId?: string | null;
    openRequestNonce?: number;
    onInsertAssetSpaceItem: (item: AssetSpaceItem) => void | Promise<void>;
    onFocusCanvasMedia: (nodeId: string) => void;
};

type SourceFilter = AssetSpaceSource | "all";
type KindFilter = AssetSpaceKind | "all";
type DramaCategoryFilter = AssetSpaceDramaCategory | "all";

export function CanvasAssetTray({
    nodes,
    activeNodeId,
    openRequestNonce = 0,
    onInsertAssetSpaceItem,
    onFocusCanvasMedia,
}: CanvasAssetTrayProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const reducedMotion = useReducedMotion();
    const rootRef = useRef<HTMLDivElement>(null);
    const listRef = useRef<HTMLDivElement>(null);
    const ignoreOutsideUntilRef = useRef(0);
    const [open, setOpen] = useState(false);
    const [source, setSource] = useState<SourceFilter>("all");
    const [kind, setKind] = useState<KindFilter>("all");
    const [dramaCategory, setDramaCategory] = useState<DramaCategoryFilter>("all");
    const [keyword, setKeyword] = useState("");
    const [insertingId, setInsertingId] = useState<string | null>(null);
    const { items, counts, loading, loadingDramaPlates } = useCanvasAssetSpace(nodes, open);
    const kindCounts = useAssetSpaceKindCounts(items, source, source === "drama" ? dramaCategory : "all");
    const dramaCategoryCounts = useAssetSpaceDramaCategoryCounts(items);
    const filtered = useFilteredAssetSpace(items, source, kind, keyword, source === "drama" ? dramaCategory : "all");
    const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
    const rowCount = Math.ceil(filtered.length / GRID_COLUMNS);
    const virtualizer = useVirtualizer({
        count: rowCount,
        getScrollElement: () => listRef.current,
        estimateSize: () => GRID_ROW_ESTIMATE,
        overscan: 6,
    });
    const motionEnabled = !reducedMotion;
    const totalCount = items.length;

    useEffect(() => {
        if (!openRequestNonce) return;
        ignoreOutsideUntilRef.current = Date.now() + 400;
        setOpen(true);
    }, [openRequestNonce]);

    useEffect(() => {
        if (!open) return;
        const closeTray = (event: PointerEvent) => {
            if (Date.now() < ignoreOutsideUntilRef.current) return;
            const target = event.target instanceof Element ? event.target : null;
            if (target && (rootRef.current?.contains(target) || target.closest("[data-canvas-no-zoom]"))) return;
            setOpen(false);
        };
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key === "Escape") setOpen(false);
        };
        document.addEventListener("pointerdown", closeTray, true);
        document.addEventListener("keydown", closeOnEscape);
        return () => {
            document.removeEventListener("pointerdown", closeTray, true);
            document.removeEventListener("keydown", closeOnEscape);
        };
    }, [open]);

    const handleSelect = async (item: AssetSpaceItem) => {
        if (item.action.type === "focus") {
            onFocusCanvasMedia(item.action.nodeId);
            return;
        }
        if (insertingId) return;
        setInsertingId(item.id);
        try {
            await onInsertAssetSpaceItem(item);
        } finally {
            setInsertingId(null);
        }
    };

    return (
        <AnimatePresence>
            {open ? (
                <div
                    ref={rootRef}
                    data-canvas-no-zoom
                    className="pointer-events-none absolute inset-0 z-[var(--z-panel)]"
                    onPointerDown={(event) => event.stopPropagation()}
                    onMouseDown={(event) => event.stopPropagation()}
                    onWheel={(event) => event.stopPropagation()}
                >
                    <motion.div
                        initial={motionEnabled ? { opacity: 0, x: -18 } : { opacity: 1 }}
                        animate={{ opacity: 1, x: 0 }}
                        exit={motionEnabled ? { opacity: 0, x: -12 } : { opacity: 0 }}
                        transition={aceternityMotion.spring.panel}
                        className="pointer-events-auto absolute bottom-[calc(var(--canvas-inset-y)+72px)] left-[var(--canvas-inset-x)] top-[var(--canvas-topbar-offset)] w-[min(92vw,372px)] origin-top-left"
                    >
                        <CanvasOverlay theme={theme} className="flex h-full min-h-0 flex-col overflow-hidden p-3" role="dialog" aria-label={canvasT("videoCanvas.asset.trayTitle", "素材空间")}>
                            <div className="flex items-start justify-between gap-2 pb-3">
                                <div className="flex min-w-0 items-center gap-2.5">
                                    <span className="grid size-9 shrink-0 place-items-center rounded-[10px] border" style={{ background: theme.spatial.surface, borderColor: theme.toolbar.border, color: theme.accent.primary }}>
                                        <FolderOpen className="size-4" />
                                    </span>
                                    <span className="min-w-0">
                                        <span className="block text-[13px] font-semibold tracking-tight">{canvasT("videoCanvas.asset.trayTitle", "素材空间")}</span>
                                        <span className="mt-0.5 block truncate text-[11px] leading-4" style={{ color: theme.node.muted }}>
                                            {canvasT("videoCanvas.asset.trayHint", "当前画布、短剧、视频生成与资讯播报的媒体")}
                                        </span>
                                    </span>
                                </div>
                                <CanvasChromeButton className="is-icon shrink-0" onClick={() => setOpen(false)} aria-label={canvasT("videoCanvas.asset.collapseTray", "收起素材空间")}>
                                    <X className="size-3.5" />
                                </CanvasChromeButton>
                            </div>

                            <div className="hover-scrollbar flex gap-1 overflow-x-auto pb-1" role="tablist" aria-label={canvasT("videoCanvas.asset.sourceAria", "按来源筛选")}>
                                <FilterChip pressed={source === "all"} onClick={() => setSource("all")} icon={<Images className="size-3" />} label={canvasT("videoCanvas.asset.sourceAll", "全部")} count={totalCount} />
                                {ASSET_SPACE_SOURCES.map((value) => (
                                    <FilterChip
                                        key={value}
                                        pressed={source === value}
                                        onClick={() => setSource(value)}
                                        icon={sourceIcon(value)}
                                        label={sourceLabel(value)}
                                        count={counts[value]}
                                    />
                                ))}
                            </div>

                            {source === "drama" ? (
                                <div className="mt-2 flex gap-1 overflow-x-auto" role="group" aria-label={canvasT("videoCanvas.asset.dramaCategoryAria", "短剧资产分类")}>
                                    <FilterChip pressed={dramaCategory === "all"} onClick={() => setDramaCategory("all")} icon={<Images className="size-3" />} label={canvasT("videoCanvas.asset.kindAll", "全部")} count={counts.drama} />
                                    {ASSET_SPACE_DRAMA_CATEGORIES.map((value) => (
                                        <FilterChip
                                            key={value}
                                            pressed={dramaCategory === value}
                                            onClick={() => setDramaCategory(value)}
                                            icon={dramaCategoryIcon(value)}
                                            label={dramaCategoryLabel(value)}
                                            count={dramaCategoryCounts[value]}
                                        />
                                    ))}
                                </div>
                            ) : null}

                            <div className="mt-2 flex gap-1" role="group" aria-label={canvasT("videoCanvas.asset.kindAria", "按类型筛选")}>
                                {(["all", "image", "video", "audio"] as const).map((value) => (
                                    <FilterChip
                                        key={value}
                                        pressed={kind === value}
                                        onClick={() => setKind(value)}
                                        icon={kindIcon(value)}
                                        label={kindLabel(value)}
                                        count={kindCounts[value]}
                                    />
                                ))}
                            </div>

                            <label className="mt-2 flex h-8 items-center gap-1.5 rounded-[10px] border px-2.5 focus-within:ring-2" style={{ background: theme.spatial.surface, borderColor: theme.toolbar.border }}>
                                <Search className="size-3.5 shrink-0" style={{ color: theme.node.muted }} />
                                <input type="search" value={keyword} onChange={(event) => setKeyword(event.target.value)} placeholder={canvasT("videoCanvas.asset.searchPlaceholder", "搜索图片 / 视频 / 音频…")} className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:opacity-55" aria-label={canvasT("videoCanvas.asset.searchAria", "搜索素材")} />
                                {keyword ? (
                                    <button type="button" className="grid size-6 shrink-0 place-items-center rounded-full opacity-55 hover:opacity-100" onClick={() => setKeyword("")} aria-label={canvasT("videoCanvas.asset.clearSearch", "清空搜索")}>
                                        <X className="size-3" />
                                    </button>
                                ) : null}
                            </label>

                            <div ref={listRef} className="hover-scrollbar mt-2.5 min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
                                {filtered.length ? (
                                    <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
                                        {virtualizer.getVirtualItems().map((virtualRow) => {
                                            const start = virtualRow.index * GRID_COLUMNS;
                                            const rowItems = filtered.slice(start, start + GRID_COLUMNS);
                                            return (
                                                <div key={virtualRow.key} className="absolute inset-x-0 grid grid-cols-2 gap-2" style={{ top: virtualRow.start, height: virtualRow.size }}>
                                                    {rowItems.map((item) => (
                                                        <AssetSpaceCard
                                                            key={item.id}
                                                            item={item}
                                                            node={item.preview.type === "node" ? nodeById.get(item.preview.nodeId) : undefined}
                                                            active={item.action.type === "focus" && item.action.nodeId === activeNodeId}
                                                            inserting={insertingId === item.id}
                                                            onSelect={() => void handleSelect(item)}
                                                        />
                                                    ))}
                                                </div>
                                            );
                                        })}
                                    </div>
                                ) : (
                                    <TrayEmpty loading={loading || loadingDramaPlates} source={source} category={dramaCategory} query={keyword} theme={theme} />
                                )}
                            </div>

                            <div className="flex items-center justify-between gap-2 px-0.5 pt-2.5 text-[11px]" style={{ color: theme.node.muted }}>
                                <span className="min-w-0 truncate">
                                    {loading || loadingDramaPlates
                                        ? canvasT("videoCanvas.asset.loadingDramaPlates", "正在同步人物、环境与道具")
                                        : source === "canvas"
                                            ? canvasT("videoCanvas.asset.hintLocate", "点击回到节点")
                                            : canvasT("videoCanvas.asset.hintInsertRemote", "画布素材定位，其余点击插入")}
                                </span>
                                <span className="shrink-0 rounded-full border px-2 py-0.5 tabular-nums" style={{ background: theme.spatial.surface, borderColor: theme.toolbar.border }}>
                                    {canvasT("videoCanvas.asset.itemCount", "{{count}} 项", { count: filtered.length })}
                                </span>
                            </div>
                        </CanvasOverlay>
                    </motion.div>
                </div>
            ) : null}
        </AnimatePresence>
    );
}

function FilterChip({ pressed, onClick, icon, label, count }: { pressed: boolean; onClick: () => void; icon: ReactNode; label: string; count: number }) {
    return (
        <CanvasChromeButton
            className="h-7 shrink-0 gap-1 px-2 text-[11px] font-medium"
            aria-pressed={pressed}
            onClick={onClick}
        >
            {icon}
            <span>{label}</span>
            <span className="tabular-nums opacity-55">{count}</span>
        </CanvasChromeButton>
    );
}

function AssetSpaceCard({
    item,
    node,
    active,
    inserting,
    onSelect,
}: {
    item: AssetSpaceItem;
    node?: CanvasNodeData;
    active: boolean;
    inserting: boolean;
    onSelect: () => void;
}) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const vimax = useVimaxAssetPreview(item.preview.type === "vimax" ? item.preview.sessionId : undefined, item.preview.type === "vimax" ? item.preview.path : undefined);
    const briefing = useBriefingAssetPreview(item.preview.type === "briefing" ? item.preview.sessionId : undefined, item.preview.type === "briefing" ? item.preview.path : undefined);
    const previewUrl = item.kind === "audio"
        ? ""
        : item.preview.type === "http"
            ? item.preview.url
            : item.preview.type === "node" && node
                ? canvasNodeDisplayUrl(node)
                : vimax.url || briefing || "";
    const [previewFailed, setPreviewFailed] = useState(false);
    useEffect(() => {
        setPreviewFailed(false);
    }, [previewUrl]);
    const KindIcon = item.kind === "video" ? Video : item.kind === "audio" ? Music2 : ImageIcon;
    const showPreview = Boolean(previewUrl) && !previewFailed && item.kind !== "audio";
    const locate = item.action.type === "focus";

    return (
        <button
            type="button"
            disabled={inserting}
            className="group flex h-[156px] flex-col overflow-hidden rounded-[12px] border text-left outline-none transition-opacity focus-visible:ring-2 disabled:opacity-60"
            style={{ background: active ? theme.accent.primarySoft : theme.spatial.surface, borderColor: active ? theme.spatial.glowStrong : theme.toolbar.border, color: theme.node.text }}
            onClick={onSelect}
            title={locate ? canvasT("videoCanvas.asset.clickLocate", "点击定位") : canvasT("videoCanvas.asset.clickInsert", "点击插入")}
        >
            <span className="relative block aspect-[16/10] w-full overflow-hidden" style={{ background: theme.node.fill }}>
                {showPreview && item.kind === "video" ? (
                    <video key={previewUrl} src={previewUrl} muted playsInline preload="metadata" className="size-full object-cover" draggable={false} onError={() => setPreviewFailed(true)} />
                ) : showPreview && item.kind === "image" ? (
                    <img key={previewUrl} src={previewUrl} alt="" width={168} height={105} loading="lazy" decoding="async" className="size-full object-cover" draggable={false} onError={() => setPreviewFailed(true)} />
                ) : (
                    <span className="grid size-full place-items-center opacity-45">
                        <KindIcon className="size-6" />
                    </span>
                )}
                <span className="absolute left-1.5 top-1.5 inline-flex items-center gap-1 rounded-full px-1.5 py-0.5 text-[10px] font-medium backdrop-blur-md" style={{ background: "color-mix(in srgb, black 42%, transparent)", color: "#fff" }}>
                    <KindIcon className="size-2.5" />
                    {item.category ? dramaCategoryLabel(item.category) : kindLabel(item.kind)}
                </span>
                <span className="absolute bottom-1.5 right-1.5 grid size-6 place-items-center rounded-full opacity-0 transition-opacity group-hover:opacity-100" style={{ background: "color-mix(in srgb, black 48%, transparent)", color: "#fff" }}>
                    {locate ? <Crosshair className="size-3" /> : <Plus className="size-3" />}
                </span>
            </span>
            <span className="min-w-0 flex-1 px-2 py-1.5">
                <span className="block truncate text-[12px] font-semibold leading-4">{item.title}</span>
                <span className="mt-0.5 block truncate text-[10px] leading-4 opacity-50">
                    {item.subtitle || sourceLabel(item.source)}
                    {inserting ? ` · ${canvasT("videoCanvas.asset.inserting", "加入中")}` : ""}
                </span>
            </span>
        </button>
    );
}

function TrayEmpty({ loading, source, category, query, theme }: { loading: boolean; source: SourceFilter; category: DramaCategoryFilter; query: string; theme: (typeof canvasThemes)[keyof typeof canvasThemes] }) {
    const text = loading
        ? source === "drama"
            ? canvasT("videoCanvas.asset.loadingDramaPlates", "正在同步人物、环境与道具")
            : canvasT("videoCanvas.asset.loadingRemote", "正在同步短剧、成片与播报")
        : query.trim()
            ? canvasT("videoCanvas.asset.noMatch", "没有匹配的素材")
            : emptyCopy(source, category);
    return (
        <div className="grid h-full min-h-[220px] place-items-center rounded-[12px] border border-dashed px-6 text-center" style={{ background: theme.spatial.surface, borderColor: theme.toolbar.border, color: theme.node.muted }}>
            <span>
                <Images className="mx-auto size-6 opacity-30" />
                <span className="mt-2.5 block text-[12px] leading-5 opacity-70">{text}</span>
            </span>
        </div>
    );
}

function sourceLabel(source: AssetSpaceSource) {
    if (source === "canvas") return canvasT("videoCanvas.asset.sourceCanvas", "当前画布");
    if (source === "drama") return canvasT("videoCanvas.asset.sourceDrama", "短剧");
    if (source === "generate") return canvasT("videoCanvas.asset.sourceGenerate", "视频生成");
    return canvasT("videoCanvas.asset.sourceBriefing", "资讯播报");
}

function sourceIcon(source: AssetSpaceSource) {
    if (source === "canvas") return <FolderOpen className="size-3" />;
    if (source === "drama") return <Clapperboard className="size-3" />;
    if (source === "generate") return <Sparkles className="size-3" />;
    return <Megaphone className="size-3" />;
}

function kindLabel(kind: KindFilter) {
    if (kind === "video") return canvasT("videoCanvas.asset.kindVideo", "视频");
    if (kind === "audio") return canvasT("videoCanvas.asset.kindAudio", "音频");
    if (kind === "image") return canvasT("videoCanvas.asset.kindImage", "图片");
    return canvasT("videoCanvas.asset.kindAll", "全部");
}

function kindIcon(kind: KindFilter) {
    if (kind === "video") return <Video className="size-3" />;
    if (kind === "audio") return <Music2 className="size-3" />;
    if (kind === "image") return <ImageIcon className="size-3" />;
    return <Images className="size-3" />;
}

function dramaCategoryLabel(category: AssetSpaceDramaCategory) {
    if (category === "character") return canvasT("videoCanvas.asset.categoryCharacter", "人物");
    if (category === "environment") return canvasT("videoCanvas.asset.categoryEnvironment", "环境");
    if (category === "prop") return canvasT("videoCanvas.asset.categoryProp", "道具");
    return canvasT("videoCanvas.asset.categoryFilm", "成片");
}

function dramaCategoryIcon(category: AssetSpaceDramaCategory) {
    if (category === "character") return <UserRound className="size-3" />;
    if (category === "environment") return <MapPinned className="size-3" />;
    if (category === "prop") return <Box className="size-3" />;
    return <Clapperboard className="size-3" />;
}

function emptyCopy(source: SourceFilter, category: DramaCategoryFilter) {
    if (source === "canvas") return canvasT("videoCanvas.asset.emptyCanvas", "这张画布还没有图片、视频或音频");
    if (source === "drama" && category === "character") return canvasT("videoCanvas.asset.emptyDramaCharacter", "还没有人物图片");
    if (source === "drama" && category === "environment") return canvasT("videoCanvas.asset.emptyDramaEnvironment", "还没有环境图片");
    if (source === "drama" && category === "prop") return canvasT("videoCanvas.asset.emptyDramaProp", "还没有道具图片");
    if (source === "drama" && category === "film") return canvasT("videoCanvas.asset.emptyDramaFilm", "还没有封面或成片");
    if (source === "drama") return canvasT("videoCanvas.asset.emptyDrama", "还没有短剧素材");
    if (source === "generate") return canvasT("videoCanvas.asset.emptyGenerate", "还没有独立成片");
    if (source === "briefing") return canvasT("videoCanvas.asset.emptyBriefing", "还没有播报成片");
    return canvasT("videoCanvas.asset.emptyAll", "还没有可展示的素材");
}
