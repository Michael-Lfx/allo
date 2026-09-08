import { useTranslation } from "react-i18next";
import { type ReactNode, useState } from "react";

import { CanvasToggle } from "@oc/components/canvas/canvas-overlay";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { type CanvasTheme } from "@oc/lib/canvas-theme";
import { imageCapabilityConfigFor, persistImageAspectValue, type ImageAspectOption, type ImageCapabilityConfig } from "@oc/lib/model-capabilities";
import { type AiConfig } from "@oc/stores/use-config-store";
import { AspectChoice, ChoiceChip, SettingsPanelHeader, SettingsSection } from "./generation-settings-chrome";

function qualityOptions() {
    return [
        { value: "auto", label: canvasT("videoCanvas.settings.qualityAuto", "自动") },
        { value: "high", label: canvasT("videoCanvas.settings.qualityHigh", "高") },
        { value: "medium", label: canvasT("videoCanvas.settings.qualityMedium", "中") },
        { value: "low", label: canvasT("videoCanvas.settings.qualityLow", "低") },
    ];
}
const DIMENSION_STEP = 16;

type ImageSettingsPanelProps = {
    config: AiConfig;
    onConfigChange: (key: "quality" | "size" | "transparentBackground" | "count", value: string) => void;
    theme: CanvasTheme;
    showTitle?: boolean;
    showCount?: boolean;
    className?: string;
    maxCount?: number;
    quickCount?: number;
};

export function ImageSettingsPanel({ config, onConfigChange, theme, showTitle = true, showCount = true, className = "w-[304px] space-y-2.5 rounded-2xl px-1 py-0.5", maxCount = 15, quickCount = 3 }: ImageSettingsPanelProps) {
    useTranslation();
    const [snapDimensionToStep, setSnapDimensionToStep] = useState(true);
    const profile = imageCapabilityConfigFor(config, config.model);
    const aspectOptions = profile.aspects;
    const quality = profile.qualities.includes(config.quality || "") ? config.quality : profile.defaultQuality;
    const transparentBackground = profile.transparentBackground && config.transparentBackground === "true";
    const countLimit = Math.min(maxCount, profile.maxCount);
    const count = Math.max(1, Math.min(countLimit, Math.floor(Math.abs(Number(config.count)) || 1)));
    const activeSize = config.size || profile.defaultSize;
    const selectedAspect = findSelectedAspect(aspectOptions, activeSize);
    const dimensions = readSizeDimensions(activeSize, selectedAspect || aspectOptions[0]);
    const selectAspect = (value: string) => {
        const option = aspectOptions.find((item) => item.value === value);
        onConfigChange("size", option ? persistImageAspectValue(option) : profile.defaultSize);
    };
    const updateDimension = (key: "width" | "height", value: number | null) => {
        const next = Math.max(1, Math.floor(value || dimensions[key] || 1024));
        const width = key === "width" ? next : dimensions.width;
        const height = key === "height" ? next : dimensions.height;
        onConfigChange("size", `${alignDimension(width, snapDimensionToStep)}x${alignDimension(height, snapDimensionToStep)}`);
    };

    return (
        <ImageSettingsTheme theme={theme}>
            <div
                className={className}
                style={{ color: theme.node.text }}
                onMouseDown={(event) => {
                    event.stopPropagation();
                    if (event.target instanceof HTMLInputElement) return;
                    if (document.activeElement instanceof HTMLInputElement && event.currentTarget.contains(document.activeElement)) document.activeElement.blur();
                }}
            >
                {showTitle ? <SettingsPanelHeader title={canvasT("videoCanvas.settings.imageTitle", "图像设置")} subtitle={canvasT("videoCanvas.settings.imageSubtitle", "画质、画幅与张数会写入这次生成。")} theme={theme} /> : null}
                {profile.qualities.length ? (
                    <SettingsSection title={canvasT("videoCanvas.settings.quality", "质量")} theme={theme}>
                        <div className="grid grid-cols-4 gap-1.5">
                            {qualityOptions().filter((item) => profile.qualities.includes(item.value)).map((item) => (
                                <ChoiceChip key={item.value} selected={quality === item.value} theme={theme} onClick={() => onConfigChange("quality", item.value)}>
                                    {item.label}
                                </ChoiceChip>
                            ))}
                        </div>
                    </SettingsSection>
                ) : null}
                {profile.transparentBackground ? (
                    <SettingsSection title={canvasT("videoCanvas.settings.transparentBg", "透明背景")} hint={canvasT("videoCanvas.settings.transparentHint", "请求模型输出保留 Alpha 通道的 PNG")} extra={(
                        <span title={canvasT("videoCanvas.settings.transparentSupportHint", "是否支持透明背景由当前模型接口决定")} onMouseDown={(event) => event.stopPropagation()}>
                            <CanvasToggle
                                theme={theme}
                                checked={transparentBackground}
                                ariaLabel={canvasT("videoCanvas.settings.transparentBg", "透明背景")}
                                onChange={(checked) => onConfigChange("transparentBackground", checked ? "true" : "false")}
                            />
                        </span>
                    )} theme={theme} />
                ) : null}
                {profile.customPixels ? (
                    <SettingsSection
                        title={canvasT("videoCanvas.settings.size", "尺寸")}
                        extra={(
                            <div className="flex items-center gap-2">
                                <span className="text-[11px] font-medium" style={{ color: theme.node.muted }}>
                                    {canvasT("videoCanvas.settings.size16Hint", "16倍数对齐")}
                                </span>
                                <span title={canvasT("videoCanvas.settings.size16AutoHint", "输入完成后自动向上补成 16 的倍数")} onMouseDown={(event) => event.stopPropagation()}>
                                    <CanvasToggle theme={theme} checked={snapDimensionToStep} onChange={setSnapDimensionToStep} ariaLabel={canvasT("videoCanvas.settings.size16Hint", "16倍数对齐")} />
                                </span>
                            </div>
                        )}
                        theme={theme}
                    >
                        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-1.5">
                            <DimensionInput prefix="W" value={dimensions.width} disabled={activeSize === "auto"} theme={theme} alignToStep={snapDimensionToStep} onChange={(value) => updateDimension("width", value)} />
                            <span className="text-sm opacity-45">×</span>
                            <DimensionInput prefix="H" value={dimensions.height} disabled={activeSize === "auto"} theme={theme} alignToStep={snapDimensionToStep} onChange={(value) => updateDimension("height", value)} />
                        </div>
                    </SettingsSection>
                ) : null}
                <SettingsSection title={canvasT("videoCanvas.settings.aspect", "宽高比")} theme={theme}>
                    <div className="grid grid-cols-4 gap-1.5 min-[380px]:grid-cols-5">
                        {aspectOptions.map((item) => (
                            <AspectChoice
                                key={item.value}
                                selected={selectedAspect?.value === item.value}
                                label={item.label}
                                theme={theme}
                                preview={<AspectIcon type={item.icon} width={item.width} height={item.height} color={theme.node.text} selected={selectedAspect?.value === item.value} accent={theme.node.activeStroke} />}
                                onClick={() => selectAspect(item.value)}
                            />
                        ))}
                    </div>
                </SettingsSection>
                {showCount ? (
                    <SettingsSection title={canvasT("videoCanvas.settings.genCount", "生成张数")} theme={theme}>
                        <div className="grid grid-cols-4 gap-1.5">
                            {Array.from({ length: Math.min(quickCount, countLimit) }, (_, index) => index + 1).map((value) => (
                                <ChoiceChip key={value} selected={count === value} theme={theme} onClick={() => onConfigChange("count", String(value))}>
                                    {value}
                                </ChoiceChip>
                            ))}
                            {countLimit > quickCount ? <CountInput value={count} quickCount={quickCount} max={countLimit} theme={theme} onChange={(value) => onConfigChange("count", String(value || 1))} /> : null}
                        </div>
                    </SettingsSection>
                ) : null}
            </div>
        </ImageSettingsTheme>
    );
}

export function ImageSettingsTheme({ children }: { theme: CanvasTheme; children: ReactNode }) {
    return <>{children}</>;
}

export function imageQualityLabel(value: string) {
    return ({ auto: canvasT("videoCanvas.settings.qualityAuto", "自动"), high: canvasT("videoCanvas.settings.qualityHigh", "高"), medium: canvasT("videoCanvas.settings.qualityMedium", "中"), low: canvasT("videoCanvas.settings.qualityLow", "低") } as Record<string, string>)[value] || canvasT("videoCanvas.settings.qualityDefault", "默认");
}

export function imageSizeLabel(size: string, profile?: ImageCapabilityConfig) {
    const aspects = profile?.aspects || [];
    return aspects.find((item) => persistImageAspectValue(item) === size || item.value === size)?.label || size;
}

function findSelectedAspect(aspects: ImageAspectOption[], size: string) {
    return aspects.find((item) => persistImageAspectValue(item) === size || item.value === size);
}

function DimensionInput({ prefix, value, disabled, theme, alignToStep, onChange }: { prefix: string; value: number; disabled: boolean; theme: CanvasTheme; alignToStep: boolean; onChange: (value: number | null) => void }) {
    const commit = (input: HTMLInputElement) => {
        const next = alignDimension(Math.max(1, Math.floor(Number(input.value) || value || 1024)), alignToStep);
        input.value = String(next);
        onChange(next);
    };

    return (
        <label className="flex h-8 overflow-hidden rounded-lg border text-xs" style={{ background: theme.canvas.background, borderColor: theme.node.stroke, color: theme.node.text, opacity: disabled ? 0.55 : 1 }}>
            <span className="grid w-8 place-items-center" style={{ color: theme.node.muted }}>
                {prefix}
            </span>
            <input
                type="number"
                min={1}
                disabled={disabled}
                className="min-w-0 flex-1 bg-transparent px-2 outline-none [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                defaultValue={value || ""}
                key={`${prefix}-${value}`}
                onBlur={(event) => commit(event.currentTarget)}
                onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                }}
                onMouseDown={(event) => event.stopPropagation()}
            />
        </label>
    );
}

function CountInput({ value, quickCount, max, theme, onChange }: { value: number; quickCount: number; max: number; theme: CanvasTheme; onChange: (value: number | null) => void }) {
    const commit = (input: HTMLInputElement) => {
        const next = Math.max(1, Math.min(max, Math.floor(Number(input.value) || 1)));
        input.value = String(next);
        onChange(next);
    };
    return (
        <label className="flex h-8 overflow-hidden rounded-full border text-xs" style={{ background: theme.canvas.background, borderColor: theme.node.stroke, color: theme.node.text }}>
            <input
                key={value > quickCount ? `custom-${value}` : "quick"}
                type="number"
                min={1}
                max={max}
                aria-label={canvasT("videoCanvas.settings.customCountAria", "自定义生成张数")}
                placeholder={canvasT("videoCanvas.settings.inputPlaceholder", "输入")}
                className="min-w-0 flex-1 bg-transparent px-2 text-center outline-none placeholder:text-current placeholder:opacity-55 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
                style={{ color: theme.node.text, WebkitTextFillColor: theme.node.text }}
                defaultValue={value > quickCount ? value : ""}
                onBlur={(event) => commit(event.currentTarget)}
                onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                }}
                onMouseDown={(event) => event.stopPropagation()}
            />
        </label>
    );
}

function AspectIcon({ type, width, height, color, selected, accent }: { type: string; width: number; height: number; color: string; selected?: boolean; accent?: string }) {
    if (type === "auto") return <span className="text-[10px] opacity-60">A</span>;
    const ratio = width / Math.max(1, height);
    const boxWidth = ratio >= 1 ? 22 : Math.max(9, 22 * ratio);
    const boxHeight = ratio >= 1 ? Math.max(9, 22 / ratio) : 22;
    return (
        <span className="grid h-6 w-8 place-items-center">
            <span className="rounded-[3px] border-2" style={{ width: boxWidth, height: boxHeight, borderColor: selected ? accent || color : color, background: selected ? `${accent || color}22` : "transparent" }} />
        </span>
    );
}

function readSizeDimensions(size: string, fallback: { width: number; height: number }) {
    const match = size?.match(/^(\d+)x(\d+)$/);
    return {
        width: match ? Number(match[1]) : fallback.width,
        height: match ? Number(match[2]) : fallback.height,
    };
}

function alignDimension(value: number, enabled: boolean) {
    return enabled ? Math.ceil(value / DIMENSION_STEP) * DIMENSION_STEP : value;
}
