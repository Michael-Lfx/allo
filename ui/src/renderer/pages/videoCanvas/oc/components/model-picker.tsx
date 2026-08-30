import { useTranslation } from "react-i18next";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Check, ChevronDown, Coins, Cpu } from "lucide-react";
import { Popover } from "antd";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes, type CanvasTheme } from "@oc/lib/canvas-theme";
import { modelCapabilityConfigFor, videoDurationOptions } from "@oc/lib/model-capabilities";
import { cn } from "@oc/lib/utils";
import { modelDisplayName, modelIconUrl, modelOptionLabel, modelOptionName, resolveModelChannel, selectableModelsByCapability, type AiConfig, type ModelCapability } from "@oc/stores/use-config-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { useUserStore } from "@oc/stores/use-user-store";
import { isMonochromeLogo, resolveModelFallbackIcon } from "@renderer/pages/videoCanvas/lib/catalogIcon";

type ModelPickerProps = {
    config: AiConfig;
    value?: string;
    onChange: (model: string) => void;
    capability?: ModelCapability;
    className?: string;
    fullWidth?: boolean;
    placeholder?: string;
    onMissingConfig?: () => void;
    showSelectedPrice?: boolean;
    variant?: "default" | "creation";
};

export function ModelPicker({ config, value, onChange, capability, className, fullWidth = false, placeholder, onMissingConfig, showSelectedPrice = true, variant = "default" }: ModelPickerProps) {
    useTranslation();
    const resolvedPlaceholder = placeholder ?? canvasT("videoCanvas.model.placeholder", "选择模型");
    const creditsEnabled = useUserStore((state) => state.features.creditsEnabled);
    const pickerId = useId();
    // 双保险：即使 store merge 写出非法 theme，这里也兜底到 dark，避免 "reading 'node'" 崩溃
    const rawTheme = useThemeStore((state) => state.theme);
    const theme = (canvasThemes[rawTheme as keyof typeof canvasThemes] ?? canvasThemes.light) as CanvasTheme;
    const [open, setOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);
    const triggerRef = useRef<HTMLButtonElement>(null);
    const options = useMemo(() => {
        const filtered = selectableModelsByCapability(config, capability);
        const current = value?.trim();
        const currentIncluded = current ? filtered.includes(current) : true;
        return Array.from(new Set([...filtered, ...(!currentIncluded && current ? [current] : [])].filter((model): model is string => Boolean(model))));
    }, [capability, config, value]);
    const optionGroups = useMemo(() => {
        const channelGroups = config.channels
            .map((channel) => ({
                key: channel.id,
                label: channel.name || canvasT("videoCanvas.model.unnamedChannel", "未命名渠道"),
                models: options.filter((model) => resolveModelChannel(config, model).id === channel.id),
            }))
            .filter((group) => group.models.length);
        const groupedModels = new Set(channelGroups.flatMap((group) => group.models));
        const ungroupedModels = options.filter((model) => !groupedModels.has(model));
        return ungroupedModels.length ? [...channelGroups, { key: "ungrouped", label: canvasT("videoCanvas.model.otherModels", "其他模型"), models: ungroupedModels }] : channelGroups;
    }, [config, options]);
    const current = value || "";
    const currentPrice = modelMenuPrice(config, current);
    const creationVariant = variant === "creation";

    useEffect(() => {
        const closeOtherPicker = (event: Event) => {
            if ((event as CustomEvent<string>).detail !== pickerId) setOpen(false);
        };
        window.addEventListener("model-picker-open", closeOtherPicker);
        return () => window.removeEventListener("model-picker-open", closeOtherPicker);
    }, [pickerId]);

    useEffect(() => {
        if (!open) return;
        // 画布拖拽从 pointerdown 开始，须在捕获阶段关闭 Portal 菜单，避免菜单与触发器分离。
        const closeOnOutsidePointer = (event: PointerEvent) => {
            const target = event.target;
            if (!(target instanceof Node)) return;
            if (triggerRef.current?.contains(target) || menuRef.current?.contains(target)) return;
            setOpen(false);
        };
        window.addEventListener("pointerdown", closeOnOutsidePointer, true);
        return () => window.removeEventListener("pointerdown", closeOnOutsidePointer, true);
    }, [open]);

    const setPickerOpen = (nextOpen: boolean) => {
        if (nextOpen && !options.length && config.channelMode === "local") onMissingConfig?.();
        if (nextOpen) window.dispatchEvent(new CustomEvent("model-picker-open", { detail: pickerId }));
        setOpen(nextOpen);
    };
    const focusMenuOption = (last = false) => {
        window.requestAnimationFrame(() => {
            const buttons = menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="option"]');
            const target = last ? buttons?.item((buttons?.length || 1) - 1) : buttons?.item(0);
            target?.focus();
        });
    };
    const handleTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
        if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
        event.preventDefault();
        setPickerOpen(true);
        focusMenuOption(event.key === "ArrowUp");
    };
    const handleMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.key === "Escape") {
            event.preventDefault();
            setOpen(false);
            triggerRef.current?.focus();
            return;
        }
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="option"]'));
        if (!buttons.length) return;
        event.preventDefault();
        const activeIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : event.key === "ArrowUp" ? Math.max(0, activeIndex - 1) : Math.min(buttons.length - 1, activeIndex + 1);
        buttons[nextIndex]?.focus();
    };
    const content = (
        <div
            ref={menuRef}
            data-canvas-no-zoom
            className={cn("canvas-model-picker-menu max-w-[calc(100vw-24px)]", creationVariant ? "creation-model-picker-menu w-[360px]" : "w-[var(--panel-width-compact)]")}
            style={{ background: theme.node.panel, color: theme.node.text }}
            role="listbox"
            aria-label={resolvedPlaceholder}
            onKeyDown={handleMenuKeyDown}
            onMouseDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
        >
            {creationVariant ? (
                <div className="creation-model-picker-heading">
                    <span>{canvasT("videoCanvas.model.selectModel", "选择模型")}</span>
                    {current ? <strong>{modelDisplayName(config, current)}</strong> : null}
                </div>
            ) : null}
            {optionGroups.length ? (
                optionGroups.map((group) => (
                    <section key={group.key} className="canvas-model-picker-group min-w-0 overflow-hidden">
                        <div className="canvas-model-picker-group-label" style={{ color: theme.node.muted }}>
                            <span className="truncate">{group.label}</span>
                        </div>
                        <div className="grid min-w-0 gap-1">
                            {group.models.map((model) => {
                                const selected = model === current;
                                return (
                                    <button
                                        key={model}
                                        type="button"
                                        role="option"
                                        aria-selected={selected}
                                        className="canvas-model-picker-option"
                                        style={{ background: selected ? theme.toolbar.activeBg : "transparent", color: theme.node.text }}
                                        onClick={() => {
                                            onChange(model);
                                            setOpen(false);
                                            window.requestAnimationFrame(() => triggerRef.current?.focus());
                                        }}
                                    >
                                        <ModelLabel config={config} model={model} capability={capability} theme={theme} showPrice={false} />
                                        {selected ? <Check className="canvas-model-picker-option-check" style={{ color: theme.node.activeStroke }} /> : null}
                                    </button>
                                );
                            })}
                        </div>
                    </section>
                ))
            ) : (
                <div className="canvas-model-picker-empty" style={{ color: theme.node.muted }}>
                    {emptyModelLabel(config, capability)}
                </div>
            )}
        </div>
    );

    return (
                <div className={cn(fullWidth ? "w-full min-w-0 max-w-full overflow-hidden" : "w-fit max-w-full")} onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
            <Popover
                open={open}
                onOpenChange={setPickerOpen}
                trigger="click"
                placement="bottomLeft"
                arrow={false}
                content={content}
                classNames={{
                    root: cn("canvas-model-picker-popover", creationVariant && "creation-model-picker-popover"),
                    container: cn("canvas-composer-popover-surface", creationVariant && "creation-model-picker-surface"),
                    content: "canvas-composer-popover-content",
                }}
            >
                <button
                    ref={triggerRef}
                    type="button"
                    className={cn("canvas-composer-model-picker", fullWidth ? "w-full" : "min-w-36 max-w-full", className)}
                    aria-haspopup="listbox"
                    aria-expanded={open}
                    aria-label={resolvedPlaceholder}
                    title={current ? modelOptionLabel(config, current) : resolvedPlaceholder}
                    onKeyDown={handleTriggerKeyDown}
                >
                    <span className="canvas-model-picker-label flex min-w-0 items-center gap-1.5">
                        <span className="canvas-model-picker-trigger-icon" style={{ background: theme.toolbar.itemHover }}>
                            <ModelIcon config={config} model={current} />
                        </span>
                        <span className="min-w-0 flex-1 truncate">{current ? (creationVariant ? modelDisplayName(config, current) : modelOptionLabel(config, current)) : resolvedPlaceholder}</span>
                    </span>
                    <ChevronDown className={cn("canvas-model-picker-chevron", open && "is-open")} aria-hidden="true" />
                </button>
            </Popover>
        </div>
    );
}

function emptyModelLabel(config: AiConfig, capability?: ModelCapability) {
    const label = capability === "image" ? canvasT("videoCanvas.model.capImage", "生图") : capability === "video" ? canvasT("videoCanvas.model.capVideo", "视频") : capability === "text" ? canvasT("videoCanvas.model.capText", "文本") : capability === "audio" ? canvasT("videoCanvas.model.capAudio", "音频") : "";
    if (capability && config.models.length) return canvasT("videoCanvas.model.noMatchInChannel", "当前渠道没有匹配的{{label}}模型", { label });
    return config.models.length ? canvasT("videoCanvas.model.noMatch", "暂无匹配的{{label}}模型", { label }) : canvasT("videoCanvas.model.addChannelFirst", "请先到配置里添加渠道和模型");
}

function ModelLabel({
    config,
    model,
    capability,
    theme,
    showPrice,
}: {
    config: AiConfig;
    model: string;
    capability?: ModelCapability;
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    showPrice: boolean;
}) {
    const meta = modelMenuMeta(model, capability);
    const videoProfile = capability === "video" ? modelCapabilityConfigFor(config, model).video : undefined;
    const capabilitySummary = videoProfile ? `${formatDurationSummary(videoProfile)} · ${videoProfile.resolutions.map((item) => item.toUpperCase()).join("/")}` : meta.description;
    return (
        <span className="flex w-full min-w-0 items-center gap-1.5 overflow-hidden py-0">
            <span className="grid size-6 shrink-0 place-items-center rounded-md" style={{ background: theme.toolbar.itemHover }}>
                <ModelIcon config={config} model={model} />
            </span>
            <span className="min-w-0 flex-1 overflow-hidden">
                <span className="block min-w-0 truncate text-[var(--fs-label)] font-medium leading-none">{modelDisplayName(config, model)}</span>
                <span className="mt-1 block truncate text-[var(--fs-tiny)]" style={{ color: theme.node.muted }} title={capabilitySummary}>
                    {capabilitySummary}
                </span>
            </span>
            {showPrice ? <ModelPrice price={modelMenuPrice(config, model)} /> : null}
        </span>
    );
}

function formatDurationSummary(profile: NonNullable<ReturnType<typeof modelCapabilityConfigFor>["video"]>) {
    const values = videoDurationOptions(profile);
    if (profile.duration.selection === "enum") return values.map((item) => `${item}s`).join("/");
    return `${profile.duration.min || values[0]}-${profile.duration.max || values[values.length - 1]}s`;
}

function modelMenuPrice(config: AiConfig, model: string): { value: number; unit: string } | null | undefined {
    if (!model) return undefined;
    const channel = resolveModelChannel(config, model);
    const cost = channel.modelCosts?.find((item) => item.model === modelOptionName(model));
    if (!cost) return channel.scope === "system" ? null : undefined;
    return { value: cost.unitPriceMicrocredits / 1_000_000, unit: cost.billingMode === "per_second" ? canvasT("videoCanvas.model.unitSecond", "秒") : canvasT("videoCanvas.model.unitTime", "次") };
}

function ModelPrice({ price, compact = false }: { price: { value: number; unit: string } | null | undefined; compact?: boolean }) {
    if (price === undefined) return null;
    if (price === null) return compact ? null : <span className="shrink-0 text-[var(--fs-tiny)] text-foreground/40">{canvasT("videoCanvas.model.unpriced", "未配置")}</span>;
    return (
        <span className="inline-flex shrink-0 items-center gap-0.5 text-[var(--fs-tiny)] font-bold tabular-nums text-amber-600 dark:text-amber-300" title={canvasT("videoCanvas.model.priceTitle", "每{{unit}}消耗 {{value}} 积分", { unit: price.unit, value: price.value.toLocaleString("zh-CN", { maximumFractionDigits: 6 }) })}>
            <Coins className="size-3" />
            {price.value.toLocaleString("zh-CN", { maximumFractionDigits: compact ? 3 : 6 })}/{price.unit}
        </span>
    );
}

function modelMenuMeta(model: string, capability?: ModelCapability): { description: string } {
    const name = modelOptionName(model).toLowerCase();
    if (capability === "image") {
        if (name.includes("nano banana") || name.includes("nanobanana") || name.includes("imagen")) return { description: "Gemini 高质量图片生成，适合角色和商业成片" };
        if (name.includes("nano") || name.includes("pro")) return { description: "高质量图片生成，适合角色和商业成片" };
        if (name.includes("seedream")) return { description: "快速出图，适合批量探索风格" };
        if (name.includes("gpt") || name.includes("image")) return { description: "通用图片模型，提示词理解稳定" };
        return { description: "图片生成模型" };
    }
    if (capability === "video") {
        if (name.includes("veo") || name.includes("omni flash") || name.includes("omni-flash")) return { description: "Gemini 镜头生成与图生视频，适合成片流程" };
        if (name.includes("seedance") || name.includes("sora")) return { description: "镜头生成与图生视频，适合成片流程" };
        return { description: "视频生成模型" };
    }
    if (capability === "audio") return { description: "语音、音效或音乐生成" };
    if (name.includes("claude")) return { description: "长文本、推理与创意写作" };
    if (name.includes("gemini")) return { description: "多模态理解与快速文本生成" };
    if (name.includes("deepseek")) return { description: "推理、代码和结构化文本" };
    return { description: capability === "text" ? "文本生成模型" : "当前渠道模型" };
}

export function ModelIcon({ config, model }: { config?: AiConfig; model: string }) {
    const catalogIcon = config ? modelIconUrl(config, model) : "";
    const fallbackIcon = resolveModelFallbackIcon(modelOptionName(model));
    const [src, setSrc] = useState(catalogIcon || fallbackIcon);
    useEffect(() => {
        setSrc(catalogIcon || fallbackIcon);
    }, [catalogIcon, fallbackIcon]);
    const monochrome = isMonochromeLogo(src);
    if (!src) return <Cpu className="size-3.5 shrink-0 opacity-70" />;
    return (
        <img
            src={src}
            alt=""
            referrerPolicy="no-referrer"
            className={cn("size-3.5 shrink-0 object-contain", monochrome && "dark:invert")}
            onError={() => {
                if (catalogIcon && src === catalogIcon && fallbackIcon && fallbackIcon !== catalogIcon) {
                    setSrc(fallbackIcon);
                    return;
                }
                setSrc("");
            }}
        />
    );
}

export function resolveModelIcon(model: string) {
    return resolveModelFallbackIcon(model);
}
