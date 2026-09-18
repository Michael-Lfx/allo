import { useCallback, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { LayoutGrid, Search, WandSparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { CanvasComposerPill } from "@oc/components/canvas/canvas-composer-pill";
import { CanvasStyleCoverSwatch } from "@oc/components/canvas/canvas-style-cover";
import { canvasThemes, type CanvasTheme } from "@oc/lib/canvas-theme";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { anchoredOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { craftCover, craftText, recipesForMode, RECIPE_BY_ID, RECIPE_GROUPS, recipeMediaKind, type CraftRecipe } from "@oc/lib/canvas/craft/catalog";
import { recipeToken } from "@oc/lib/canvas/craft/tokens";
import { readCraftRecents, rememberCraftRecent } from "@oc/lib/canvas/craft/recents";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { CanvasGenerationMode } from "@oc/types/canvas";

export type CanvasPromptPreset = {
    id: string;
    name: string;
    description: string;
    prompt: string;
    modes: CanvasGenerationMode[];
    source: "builtin";
};

export function CanvasPresetPicker({
    mode,
    open,
    onOpenChange,
    onSelect,
    onOpenLibrary,
}: {
    mode: CanvasGenerationMode;
    open?: boolean;
    onOpenChange?: (open: boolean) => void;
    onSelect: (preset: CanvasPromptPreset) => void;
    onOpenLibrary?: () => void;
}) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const buttonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [internalOpen, setInternalOpen] = useState(false);
    const [query, setQuery] = useState("");
    const actualOpen = open ?? internalOpen;
    const setOpen = useCallback((next: boolean) => {
        if (!next) setQuery("");
        setInternalOpen(next);
        onOpenChange?.(next);
    }, [onOpenChange]);
    const close = useCallback(() => setOpen(false), [setOpen]);
    const rect = useAnchoredOverlay(actualOpen, buttonRef, panelRef, close);
    const geometry = rect ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 400, placement: "topLeft" }) : null;
    const recipes = useMemo(() => recipesForMode(mode, query), [mode, query]);
    const recents = useMemo(() => {
        if (query.trim()) return [];
        return readCraftRecents().flatMap((id) => {
            const item = RECIPE_BY_ID.get(id);
            return item && item.modes.includes(mode) ? [item] : [];
        });
    }, [mode, query, actualOpen]);
    const recentIds = useMemo(() => new Set(recents.map((item) => item.id)), [recents]);
    const grouped = useMemo(() => {
        const rest = recents.length ? recipes.filter((item) => !recentIds.has(item.id)) : recipes;
        return RECIPE_GROUPS.map((id) => ({ id, items: rest.filter((item) => item.group === id) })).filter((section) => section.items.length);
    }, [recipes, recents, recentIds]);

    const pick = (id: string, name: string, description: string) => {
        rememberCraftRecent(id);
        onSelect({ id, name, description, prompt: recipeToken(id), modes: [mode], source: "builtin" });
        setOpen(false);
    };

    return (
        <>
            <CanvasComposerPill
                ref={buttonRef}
                className="canvas-preset-picker-trigger"
                icon={<WandSparkles />}
                label={canvasT("videoCanvas.preset.label", "手法")}
                expanded={actualOpen}
                title={canvasT("videoCanvas.preset.open", "打开手法")}
                aria-label={canvasT("videoCanvas.preset.open", "打开手法")}
                onClick={() => setOpen(!actualOpen)}
            />
            {actualOpen && geometry
                ? createPortal(
                    <div
                        ref={panelRef}
                        data-canvas-no-zoom
                        className="canvas-overlay canvas-preset-picker-menu"
                        style={overlayPanelStyle(theme, geometry)}
                        onMouseDown={(event) => event.stopPropagation()}
                        onPointerDown={(event) => event.stopPropagation()}
                    >
                        <label className="flex items-center gap-1.5 rounded-xl px-2" style={{ background: theme.toolbar.itemHover }}>
                            <Search className="size-3.5 shrink-0" style={{ color: theme.node.muted }} />
                            <input
                                className="canvas-sheet-input h-8 flex-1 border-0 bg-transparent px-0"
                                autoFocus
                                placeholder={canvasT("videoCanvas.preset.searchPlaceholder", "搜索手法")}
                                value={query}
                                onChange={(event) => setQuery(event.target.value)}
                            />
                        </label>
                        <div className="thin-scrollbar mt-2 max-h-80 space-y-1 overflow-y-auto">
                            {recents.length ? (
                                <>
                                    <div className="px-1.5 py-1 text-[10px] font-medium uppercase tracking-wide" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.preset.recent", "最近")}</div>
                                    <div className="canvas-preset-picker-recents">
                                        {recents.map((item) => (
                                            <button key={`recent-${item.id}`} type="button" className="canvas-preset-picker-recent" onClick={() => pick(item.id, craftText(item.title), craftText(item.job))}>
                                                <CanvasStyleCoverSwatch cover={craftCover(item.coverLookId)} className="aspect-[4/3] w-full rounded-xl" hoverZoom />
                                                <span className="truncate text-[11px] font-semibold" style={{ color: theme.node.text }}>{craftText(item.title)}</span>
                                            </button>
                                        ))}
                                    </div>
                                </>
                            ) : null}
                            {grouped.map((section) => (
                                <div key={section.id}>
                                    <div className="px-2 py-1 text-[10px] font-medium uppercase tracking-wide" style={{ color: theme.node.muted }}>{canvasT(`videoCanvas.craft.group.${section.id}`, section.id)}</div>
                                    {section.items.map((item) => (
                                        <RecipeRow key={item.id} theme={theme} item={item} onPick={() => pick(item.id, craftText(item.title), craftText(item.job))} />
                                    ))}
                                </div>
                            ))}
                            {recipes.length === 0 ? (
                                <div className="py-8 text-center text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.preset.noMatch", "没有匹配的手法")}</div>
                            ) : null}
                        </div>
                        {onOpenLibrary ? (
                            <button type="button" className="mt-1.5 flex w-full items-center justify-center gap-1.5 rounded-full px-2 py-2 text-[12px] font-medium" style={{ color: theme.accent.primary, background: theme.toolbar.itemHover }} onClick={() => { setOpen(false); onOpenLibrary(); }}>
                                <LayoutGrid className="size-3.5" />
                                {canvasT("videoCanvas.preset.openLibrary", "打开货架")}
                            </button>
                        ) : null}
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}

function RecipeRow({ theme, item, onPick }: { theme: CanvasTheme; item: CraftRecipe; onPick: () => void }) {
    const media = recipeMediaKind(item);
    const mediaLabel = media === "image"
        ? canvasT("videoCanvas.craft.badgeImageOnly", "图专")
        : media === "video"
            ? canvasT("videoCanvas.craft.badgeVideoOnly", "视专")
            : canvasT("videoCanvas.craft.badgeBoth", "图·视");
    return (
        <button type="button" className="canvas-preset-picker-option" onClick={onPick}>
            <CanvasStyleCoverSwatch cover={craftCover(item.coverLookId)} className="h-12 w-[76px] shrink-0 rounded-xl" hoverZoom />
            <span className="min-w-0 flex-1">
                <span className="flex items-center gap-1.5 text-[13px] font-semibold" style={{ color: theme.node.text }}>
                    <span className="truncate">{craftText(item.title)}</span>
                    <span className="shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium" style={{ color: theme.node.muted, background: theme.toolbar.itemHover }}>{mediaLabel}</span>
                </span>
                <span className="mt-0.5 block truncate text-[11px] leading-4" style={{ color: theme.node.muted }}>{craftText(item.job)}</span>
            </span>
        </button>
    );
}
