import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { Check, Eye, Palette } from "lucide-react";
import { Button, Modal, Segmented } from "antd";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import {
    compatibleProjectStyleCharacters,
    compileCanvasStylePreset,
    defaultProjectStyleSelection,
    lookbookCanvasStylePresets,
    parseCanvasStyleSelection,
    projectStyleCharacters,
    projectStyleMedia,
    projectStyleTones,
    projectStyleWorlds,
    recommendedCanvasStylePresets,
    type CanvasStylePreset,
    type ProjectStyleMediumId,
    type ProjectStyleSelection,
} from "@oc/lib/canvas/canvas-style-system";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasStyleCoverSwatch } from "./canvas-style-cover";

export type { CanvasStylePreset } from "@oc/lib/canvas/canvas-style-system";
export { canvasStylePresets, lookbookCanvasStylePresets, resolveCanvasStylePreset } from "@oc/lib/canvas/canvas-style-system";

export function CanvasStylePickerModal({ open, value, onClose, onSelect }: { open: boolean; value?: string; onClose: () => void; onSelect: (preset: CanvasStylePreset) => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [detailPreset, setDetailPreset] = useState<CanvasStylePreset | null>(null);
    const [mode, setMode] = useState<"recommended" | "custom">("recommended");
    const [lookFilter, setLookFilter] = useState("all");
    const [selection, setSelection] = useState<ProjectStyleSelection>(defaultProjectStyleSelection);
    useEffect(() => {
        if (!open) return;
        const parsed = parseCanvasStyleSelection(value);
        if (parsed) {
            setSelection(parsed);
            const key = `${parsed.world}--${parsed.tone}--${parsed.medium}--${parsed.character}`;
            const inLookbook = lookbookCanvasStylePresets.some((preset) => presetSelectionKey(preset) === key || preset.id === value);
            const inCombos = uniqueRecommendedCombos(lookbookCanvasStylePresets, recommendedCanvasStylePresets).some((preset) => presetSelectionKey(preset) === key);
            setMode(inLookbook || inCombos ? "recommended" : "custom");
        } else {
            setSelection(defaultProjectStyleSelection);
            setMode("recommended");
        }
        setLookFilter("all");
    }, [open, value]);
    const lookCategories = useMemo(() => {
        const seen = new Set<string>();
        return lookbookCanvasStylePresets.flatMap((preset) => {
            const category = localizedStylePresetDisplay(preset).category;
            if (seen.has(category)) return [];
            seen.add(category);
            return [category];
        });
    }, []);
    const uniqueComboPresets = useMemo(() => uniqueRecommendedCombos(lookbookCanvasStylePresets, recommendedCanvasStylePresets), []);
    const lookbookPresets = lookFilter === "all"
        ? lookbookCanvasStylePresets
        : lookbookCanvasStylePresets.filter((preset) => localizedStylePresetDisplay(preset).category === lookFilter);
    const comboPresets = lookFilter === "all" ? uniqueComboPresets : [];
    return (
        <>
            <Modal rootClassName="canvas-style-picker-modal" open={open} getContainer={() => document.body} zIndex={1200} title={null} footer={null} centered width="min(1080px, calc(100vw - 24px))" onCancel={onClose} styles={{ container: { padding: 0 }, body: { padding: 0 } }}>
                <div className="overflow-hidden" style={{ color: theme.node.text, background: theme.node.panel }}>
                    <header className="border-b px-5 py-4 pr-12" style={{ borderColor: theme.node.stroke }}>
                        <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                            <div className="min-w-0">
                                <p className="text-[var(--fs-tiny)] font-medium tracking-[0.14em] uppercase" style={{ color: theme.node.activeStroke }}>{canvasT("videoCanvas.stylePicker.kicker", "PROJECT LOOK")}</p>
                                <h2 className="mt-1 text-lg font-semibold tracking-tight">{canvasT("videoCanvas.stylePicker.title", "选择项目画风")}</h2>
                                <p className="mt-1.5 max-w-xl text-xs leading-5" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.stylePicker.subtitle", "为整部短剧锁定题材世界、叙事气质与影像媒介。这是全项目美术基线，不是单镜头提示词。")}</p>
                            </div>
                            <Segmented size="small" value={mode} options={[{ value: "recommended", label: canvasT("videoCanvas.stylePicker.recommended", "推荐画风") }, { value: "custom", label: canvasT("videoCanvas.stylePicker.custom", "自定义组合") }]} onChange={(next) => setMode(next as typeof mode)} />
                        </div>
                    </header>
                    {mode === "recommended" ? (
                        <div className="thin-scrollbar max-h-[76vh] overflow-y-auto px-5 py-4">
                            <div className="mb-3 flex flex-wrap items-center gap-1.5">
                                <LookFilterChip label={canvasT("videoCanvas.stylePicker.allLooks", "全部气质")} active={lookFilter === "all"} theme={theme} onClick={() => setLookFilter("all")} />
                                {lookCategories.map((category) => (
                                    <LookFilterChip key={category} label={category} active={lookFilter === category} theme={theme} onClick={() => setLookFilter(category)} />
                                ))}
                            </div>
                            <p className="mb-3 text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.stylePicker.lookbookHint", "按真人、动画、插画、手工等制作媒介组织的项目基线，点选即可锁定全片画风。需要更细的交叉组合，可切换到自定义组合。")}</p>
                            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
                                {lookbookPresets.map((preset) => (
                                    <StyleLookCard key={preset.id} preset={preset} active={isStylePresetActive(preset, value)} theme={theme} onSelect={onSelect} onDetail={setDetailPreset} />
                                ))}
                            </div>
                            {comboPresets.length ? (
                                <>
                                    <h3 className="mb-1 mt-8 text-xs font-semibold tracking-wide">{canvasT("videoCanvas.stylePicker.comboSection", "题材组合")}</h3>
                                    <p className="mb-3 text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.stylePicker.comboHint", "与上方气质完全相同的条目已去掉。其余交叉组合沿用同一套大卡，请以标题和标签区分。")}</p>
                                    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
                                        {comboPresets.map((preset) => (
                                            <StyleLookCard key={preset.id} preset={preset} active={isStylePresetActive(preset, value)} theme={theme} onSelect={onSelect} onDetail={setDetailPreset} />
                                        ))}
                                    </div>
                                </>
                            ) : null}
                        </div>
                    ) : <CanvasStyleComposer selection={selection} value={value} theme={theme} onChange={setSelection} onSelect={onSelect} onDetail={setDetailPreset} />}
                </div>
            </Modal>
            <CanvasStyleDetailModal open={Boolean(detailPreset)} preset={detailPreset} selected={detailPreset?.id === value} onClose={() => setDetailPreset(null)} onSelect={(preset) => { setDetailPreset(null); onSelect(preset); }} />
        </>
    );
}

function LookFilterChip({ label, active, theme, onClick }: { label: string; active: boolean; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onClick: () => void }) {
    return (
        <button
            type="button"
            className="h-7 rounded-full px-3 text-[var(--fs-tiny)] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2"
            style={{ background: active ? theme.node.activeStroke : theme.toolbar.itemHover, color: active ? theme.canvas.background : theme.node.text, "--tw-ring-color": theme.node.activeStroke } as CSSProperties}
            onClick={onClick}
        >
            {label}
        </button>
    );
}

function StyleLookCard({ preset, active, theme, onSelect, onDetail }: { preset: CanvasStylePreset; active: boolean; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onSelect: (preset: CanvasStylePreset) => void; onDetail: (preset: CanvasStylePreset) => void }) {
    const display = localizedStylePresetDisplay(preset);
    const mix = preset.selection || parseCanvasStyleSelection(preset.id);
    const chips = mix && preset.id.startsWith("v2-")
        ? [localizedStyleOptionLabel("world", mix.world), localizedStyleOptionLabel("tone", mix.tone), localizedStyleOptionLabel("medium", mix.medium)]
        : preset.tags.slice(0, 3);
    return (
        <article className="group relative">
            <button type="button" className="flex w-full flex-col overflow-hidden rounded-2xl border text-left transition-[transform,box-shadow] duration-300 hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2" style={{ background: theme.canvas.background, borderColor: active ? theme.node.activeStroke : theme.node.stroke, boxShadow: active ? `0 0 0 1px ${theme.node.activeStroke}, 0 18px 40px ${theme.spatial.shadow}` : `0 10px 28px ${theme.spatial.shadow}`, "--tw-ring-color": theme.node.activeStroke } as CSSProperties} onClick={() => onSelect(preset)}>
                <CanvasStyleCoverSwatch cover={preset.cover} className="aspect-[16/10] w-full" hoverZoom alt={canvasT("videoCanvas.stylePicker.previewAlt", "{{title}}画风示意", { title: display.title })}>
                    <span className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/70 via-black/10 to-transparent" />
                    <span className="absolute inset-x-3 bottom-3 flex items-end justify-between gap-2">
                        <span className="min-w-0">
                            <span className="block text-[10px] font-medium tracking-[0.16em] uppercase text-white/70">{display.category}</span>
                            <span className="mt-0.5 block truncate text-sm font-semibold text-white">{display.title}</span>
                        </span>
                        {active ? <span className="grid size-5 shrink-0 place-items-center rounded-full bg-white text-black"><Check className="size-3" /></span> : null}
                    </span>
                </CanvasStyleCoverSwatch>
                <span className="px-3 py-2.5">
                    <span className="line-clamp-2 block text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>{display.description}</span>
                    {chips.length ? (
                        <span className="mt-2 flex flex-wrap gap-1">
                            {chips.map((chip) => (
                                <span key={chip} className="rounded-full px-2 py-0.5 text-[10px] leading-4" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}>{chip}</span>
                            ))}
                        </span>
                    ) : null}
                </span>
            </button>
            <button type="button" className="absolute right-2.5 top-2.5 grid size-8 place-items-center rounded-full backdrop-blur-md transition-opacity focus-visible:outline-none focus-visible:ring-2" style={{ background: "rgba(0,0,0,.42)", color: "white", "--tw-ring-color": theme.node.activeStroke } as CSSProperties} onClick={() => onDetail(preset)} aria-label={canvasT("videoCanvas.stylePicker.viewSpecAria", "查看{{title}}规范", { title: display.title })} title={canvasT("videoCanvas.stylePicker.viewSpec", "查看画风规范")}><Eye className="size-3.5" /></button>
        </article>
    );
}

function uniqueRecommendedCombos(lookbook: CanvasStylePreset[], recommended: CanvasStylePreset[]) {
    const seenKeys = new Set(lookbook.flatMap((preset) => {
        const key = presetSelectionKey(preset);
        return key ? [key] : [];
    }));
    return recommended.filter((preset) => {
        const key = presetSelectionKey(preset);
        if (!key || seenKeys.has(key)) return false;
        seenKeys.add(key);
        return true;
    });
}

function presetSelectionKey(preset: CanvasStylePreset) {
    const selection = preset.selection || parseCanvasStyleSelection(preset.id);
    return selection ? `${selection.world}--${selection.tone}--${selection.medium}--${selection.character}` : "";
}

function isStylePresetActive(preset: CanvasStylePreset, value?: string) {
    if (!value) return false;
    if (preset.id === value) return true;
    const presetKey = presetSelectionKey(preset);
    if (!presetKey) return false;
    const parsed = parseCanvasStyleSelection(value);
    if (parsed) return presetKey === `${parsed.world}--${parsed.tone}--${parsed.medium}--${parsed.character}`;
    const selectedLook = lookbookCanvasStylePresets.find((item) => item.id === value);
    return Boolean(selectedLook && presetSelectionKey(selectedLook) === presetKey);
}

function CanvasStyleComposer({ selection, value, theme, onChange, onSelect, onDetail }: {
    selection: ProjectStyleSelection;
    value?: string;
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    onChange: (selection: ProjectStyleSelection) => void;
    onSelect: (preset: CanvasStylePreset) => void;
    onDetail: (preset: CanvasStylePreset) => void;
}) {
    useTranslation();
    const preset = useMemo(() => compileCanvasStylePreset(selection), [selection]);
    const display = localizedStylePresetDisplay(preset);
    const characterOptions = compatibleProjectStyleCharacters(selection.medium);
    const updateMedium = (medium: ProjectStyleMediumId) => {
        const compatible = compatibleProjectStyleCharacters(medium);
        const character = compatible.some((item) => item.id === selection.character) ? selection.character : compatible[0].id;
        onChange({ ...selection, medium, character });
    };
    return (
        <div className="thin-scrollbar grid max-h-[76vh] overflow-y-auto md:grid-cols-5">
            <section className="grid content-start gap-5 border-b p-5 md:col-span-2 md:border-b-0 md:border-r" style={{ borderColor: theme.node.stroke }}>
                <StyleChipSelect kind="world" label={canvasT("videoCanvas.stylePicker.world", "题材世界")} value={selection.world} options={projectStyleWorlds} theme={theme} onChange={(world) => onChange({ ...selection, world })} />
                <StyleChipSelect kind="tone" label={canvasT("videoCanvas.stylePicker.tone", "叙事气质")} value={selection.tone} options={projectStyleTones} theme={theme} onChange={(tone) => onChange({ ...selection, tone })} />
                <StyleChipSelect kind="medium" label={canvasT("videoCanvas.stylePicker.medium", "视觉媒介")} value={selection.medium} options={projectStyleMedia} theme={theme} onChange={updateMedium} />
                <StyleChipSelect kind="character" label={canvasT("videoCanvas.stylePicker.character", "角色造型")} value={selection.character} options={characterOptions} theme={theme} onChange={(character) => onChange({ ...selection, character })} />
            </section>
            <section className="flex min-h-0 flex-col md:col-span-3">
                <CanvasStyleCoverSwatch cover={preset.cover} className="aspect-video w-full" alt={canvasT("videoCanvas.stylePicker.previewAlt", "{{title}}画风示意", { title: display.title })}>
                    <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/55 via-transparent to-transparent" />
                    <div className="absolute inset-x-5 bottom-4">
                        <div className="text-[10px] font-medium tracking-[0.16em] uppercase text-white/70">{display.category}</div>
                        <div className="mt-1 text-base font-semibold text-white">{display.title}</div>
                    </div>
                </CanvasStyleCoverSwatch>
                <div className="flex min-h-0 flex-1 flex-col p-5">
                    <p className="text-xs leading-5" style={{ color: theme.node.muted }}>{display.description}</p>
                    <div className="mt-4 grid grid-cols-2 gap-x-4 gap-y-2 rounded-xl border px-3 py-3 text-[var(--fs-tiny)]" style={{ borderColor: theme.node.stroke, background: theme.canvas.background }}>
                        <StyleSummary label={canvasT("videoCanvas.stylePicker.summaryWorld", "题材")} value={localizedStyleOptionLabel("world", selection.world)} muted={theme.node.muted} />
                        <StyleSummary label={canvasT("videoCanvas.stylePicker.summaryTone", "气质")} value={localizedStyleOptionLabel("tone", selection.tone)} muted={theme.node.muted} />
                        <StyleSummary label={canvasT("videoCanvas.stylePicker.summaryMedium", "媒介")} value={localizedStyleOptionLabel("medium", selection.medium)} muted={theme.node.muted} />
                        <StyleSummary label={canvasT("videoCanvas.stylePicker.summaryCharacter", "角色")} value={localizedStyleOptionLabel("character", selection.character)} muted={theme.node.muted} />
                    </div>
                    <div className="mt-auto flex justify-end gap-2 pt-5">
                        <Button icon={<Eye className="size-3.5" />} onClick={() => onDetail(preset)}>{canvasT("videoCanvas.stylePicker.viewRules", "查看规范")}</Button>
                        <Button type="primary" disabled={preset.id === value} icon={preset.id === value ? <Check className="size-3.5" /> : <Palette className="size-3.5" />} onClick={() => onSelect(preset)}>{preset.id === value ? canvasT("videoCanvas.stylePicker.currentStyle", "当前画风") : canvasT("videoCanvas.stylePicker.applyCombo", "应用组合")}</Button>
                    </div>
                </div>
            </section>
        </div>
    );
}

function StyleSummary({ label, value, muted }: { label: string; value?: string; muted: string }) {
    return <div className="flex min-w-0 items-center gap-2"><span className="shrink-0" style={{ color: muted }}>{label}</span><span className="truncate font-medium">{value}</span></div>;
}

function StyleChipSelect<T extends string>({ kind, label, value, options, theme, onChange }: { kind: StyleOptionKind; label: string; value: T; options: Array<{ id: T; label: string; description: string }>; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onChange: (value: T) => void }) {
    const selected = options.find((option) => option.id === value);
    return (
        <div className="grid gap-2">
            <div className="text-xs font-semibold">{label}</div>
            <div className="flex flex-wrap gap-1.5">
                {options.map((option) => {
                    const active = option.id === value;
                    return (
                        <button
                            key={option.id}
                            type="button"
                            className="h-8 rounded-full px-3 text-[var(--fs-tiny)] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2"
                            style={{ background: active ? theme.node.activeStroke : theme.toolbar.itemHover, color: active ? theme.canvas.background : theme.node.text, "--tw-ring-color": theme.node.activeStroke } as CSSProperties}
                            onClick={() => onChange(option.id)}
                        >
                            {localizedStyleOptionLabel(kind, option.id)}
                        </button>
                    );
                })}
            </div>
            {selected ? <p className="text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>{localizedStyleOptionDesc(kind, selected.id, selected.description)}</p> : null}
        </div>
    );
}

export function CanvasStyleDetailModal({ open, preset, selected = false, onClose, onSelect }: { open: boolean; preset: CanvasStylePreset | null; selected?: boolean; onClose: () => void; onSelect?: (preset: CanvasStylePreset) => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const display = preset ? localizedStylePresetDisplay(preset) : null;
    const sections = preset ? parseStyleSections(preset.prompt) : [];
    return (
        <Modal rootClassName="canvas-style-detail-modal" open={open} getContainer={() => document.body} zIndex={1210} title={null} footer={null} centered destroyOnHidden width="min(820px, calc(100vw - 24px))" onCancel={onClose} styles={{ container: { padding: 0 }, body: { padding: 0 } }}>
            {preset && display ? <div className="flex max-h-[84vh] flex-col overflow-hidden" style={{ color: theme.node.text, background: theme.node.panel }}>
                <CanvasStyleCoverSwatch cover={preset.cover} className="h-44 w-full shrink-0 border-b sm:h-52" style={{ borderColor: theme.node.stroke }} alt={canvasT("videoCanvas.stylePicker.previewAlt", "{{title}}画风示意", { title: display.title })} />
                <header className="border-b px-4 py-3 pr-12 sm:px-5 sm:pr-12" style={{ borderColor: theme.node.stroke }}>
                    <div className="text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.activeStroke }}>{display.category}</div>
                    <h2 className="mt-1 text-base font-semibold">{display.title}</h2>
                    <p className="mt-1.5 text-xs leading-5" style={{ color: theme.node.muted }}>{display.description}</p>
                </header>
                <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 sm:px-5">
                    {sections.map((section) => <section key={section.title} className="border-b py-3 last:border-b-0" style={{ borderColor: theme.node.stroke }}><h3 className="text-xs font-semibold">{section.title}</h3><p className="mt-1.5 text-xs leading-5" style={{ color: theme.node.muted }}>{section.content}</p></section>)}
                </div>
                <footer className="flex shrink-0 justify-end gap-2 border-t px-4 py-3 sm:px-5" style={{ borderColor: theme.node.stroke }}><Button onClick={onClose}>{canvasT("videoCanvas.stylePicker.close", "关闭")}</Button>{onSelect ? <Button type="primary" disabled={selected} icon={selected ? <Check className="size-3.5" /> : <Palette className="size-3.5" />} onClick={() => onSelect(preset)}>{selected ? canvasT("videoCanvas.stylePicker.currentStyle", "当前画风") : canvasT("videoCanvas.stylePicker.selectStyle", "选择该画风")}</Button> : null}</footer>
            </div> : null}
        </Modal>
    );
}

type StyleOptionKind = "world" | "tone" | "medium" | "character";

function styleOptionBucket(kind: StyleOptionKind) {
    if (kind === "world") return { labels: "worlds", descs: "worldDescs", source: projectStyleWorlds };
    if (kind === "tone") return { labels: "tones", descs: "toneDescs", source: projectStyleTones };
    if (kind === "medium") return { labels: "media", descs: "mediaDescs", source: projectStyleMedia };
    return { labels: "characters", descs: "characterDescs", source: projectStyleCharacters };
}

function localizedStyleOptionLabel(kind: StyleOptionKind, id: string) {
    const bucket = styleOptionBucket(kind);
    const fallback = bucket.source.find((item) => item.id === id)?.label || id;
    return canvasT(`videoCanvas.stylePicker.${bucket.labels}.${id}`, fallback);
}

function localizedStyleOptionDesc(kind: StyleOptionKind, id: string, fallback: string) {
    const bucket = styleOptionBucket(kind);
    return canvasT(`videoCanvas.stylePicker.${bucket.descs}.${id}`, fallback);
}

/** UI-only localization; stored preset.prompt stays Chinese for generation. */
function localizedStylePresetDisplay(preset: CanvasStylePreset) {
    const selection = preset.selection || parseCanvasStyleSelection(preset.id);
    if (selection && preset.id.startsWith("v2-")) {
        const world = localizedStyleOptionLabel("world", selection.world);
        const tone = localizedStyleOptionLabel("tone", selection.tone);
        const medium = localizedStyleOptionLabel("medium", selection.medium);
        const worldDesc = localizedStyleOptionDesc("world", selection.world, projectStyleWorlds.find((item) => item.id === selection.world)?.description || "");
        const toneDesc = localizedStyleOptionDesc("tone", selection.tone, projectStyleTones.find((item) => item.id === selection.tone)?.description || "");
        const mediumDesc = localizedStyleOptionDesc("medium", selection.medium, projectStyleMedia.find((item) => item.id === selection.medium)?.description || "");
        return {
            title: canvasT("videoCanvas.stylePicker.titleJoin", "{{world}} · {{tone}} · {{medium}}", { world, tone, medium }),
            category: canvasT("videoCanvas.stylePicker.categoryJoin", "{{world}} / {{medium}}", { world, medium }),
            description: `${worldDesc}${toneDesc}${mediumDesc}`,
        };
    }
    return {
        title: canvasT(`videoCanvas.stylePicker.legacy.${preset.id}.title`, preset.title),
        category: canvasT(`videoCanvas.stylePicker.legacy.${preset.id}.category`, preset.category),
        description: canvasT(`videoCanvas.stylePicker.legacy.${preset.id}.description`, preset.description),
    };
}

function parseStyleSections(prompt: string) {
    return prompt.split("\n").map((line) => {
        const match = line.match(/^【([^】]+)】(.*)$/);
        return match ? { title: match[1], content: match[2] } : { title: canvasT("videoCanvas.stylePicker.extraSpec", "补充规范"), content: line };
    }).filter((section) => section.content);
}
