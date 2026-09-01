import { useTranslation } from "react-i18next";
import { type ReactNode, useState } from "react";
import { ConfigProvider, Switch } from "antd";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { type CanvasTheme } from "@oc/lib/canvas-theme";
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

const aspectOptions = [
    { value: "1:1", label: "1:1", width: 1024, height: 1024, icon: "square" },
    { value: "3:2", label: "3:2", width: 1536, height: 1024, icon: "landscape" },
    { value: "2:3", label: "2:3", width: 1024, height: 1536, icon: "portrait" },
    { value: "4:3", label: "4:3", width: 1360, height: 1024, icon: "landscape" },
    { value: "3:4", label: "3:4", width: 1024, height: 1360, icon: "portrait" },
    { value: "16:9", label: "16:9", width: 1824, height: 1024, icon: "landscape" },
    { value: "21:9", label: "21:9", size: "2352x1008", width: 2352, height: 1008, icon: "landscape" },
    { value: "9:16", label: "9:16", width: 1024, height: 1824, icon: "portrait" },
    { value: "1:1-2k", label: "1:1(2k)", size: "2048x2048", width: 2048, height: 2048, icon: "square" },
    { value: "16:9-2k", label: "16:9(2k)", size: "2048x1152", width: 2048, height: 1152, icon: "landscape" },
    { value: "9:16-2k", label: "9:16(2k)", size: "1152x2048", width: 1152, height: 2048, icon: "portrait" },
    { value: "16:9-4k", label: "16:9(4k)", size: "3840x2160", width: 3840, height: 2160, icon: "landscape" },
    { value: "9:16-4k", label: "9:16(4k)", size: "2160x3840", width: 2160, height: 3840, icon: "portrait" },
    { value: "auto", label: "auto", width: 0, height: 0, icon: "auto" },
];

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
    const quality = config.quality || "auto";
    const transparentBackground = config.transparentBackground === "true";
    const count = Math.max(1, Math.min(maxCount, Math.floor(Math.abs(Number(config.count)) || 1)));
    const activeSize = config.size || "auto";
    const selectedAspect = aspectOptions.find((item) => (item.size || item.value) === activeSize || item.value === activeSize);
    const dimensions = readSizeDimensions(activeSize, selectedAspect || aspectOptions[0]);
    const selectAspect = (value: string) => {
        const option = aspectOptions.find((item) => item.value === value);
        onConfigChange("size", option?.size || option?.value || "auto");
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
                <SettingsSection title={canvasT("videoCanvas.settings.quality", "质量")} theme={theme}>
                    <div className="grid grid-cols-4 gap-1.5">
                        {qualityOptions().map((item) => (
                            <ChoiceChip key={item.value} selected={quality === item.value} theme={theme} onClick={() => onConfigChange("quality", item.value)}>
                                {item.label}
                            </ChoiceChip>
                        ))}
                    </div>
                </SettingsSection>
                <SettingsSection title={canvasT("videoCanvas.settings.transparentBg", "透明背景")} hint={canvasT("videoCanvas.settings.transparentHint", "请求模型输出保留 Alpha 通道的 PNG")} extra={(
                    <span title={canvasT("videoCanvas.settings.transparentSupportHint", "是否支持透明背景由当前模型接口决定")} onMouseDown={(event) => event.stopPropagation()}>
                        <Switch
                            size="small"
                            checked={transparentBackground}
                            onChange={(checked) => onConfigChange("transparentBackground", checked ? "true" : "false")}
                        />
                    </span>
                )} theme={theme} />
                <SettingsSection
                    title={canvasT("videoCanvas.settings.size", "尺寸")}
                    extra={(
                        <div className="flex items-center gap-2">
                            <span className="text-[11px] font-medium" style={{ color: theme.node.muted }}>
                                {canvasT("videoCanvas.settings.size16Hint", "16倍数对齐")}
                            </span>
                            <span title={canvasT("videoCanvas.settings.size16AutoHint", "输入完成后自动向上补成 16 的倍数")} onMouseDown={(event) => event.stopPropagation()}>
                                <Switch size="small" checked={snapDimensionToStep} onChange={setSnapDimensionToStep} />
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
                            {Array.from({ length: quickCount }, (_, index) => index + 1).map((value) => (
                                <ChoiceChip key={value} selected={count === value} theme={theme} onClick={() => onConfigChange("count", String(value))}>
                                    {value}
                                </ChoiceChip>
                            ))}
                            <CountInput value={count} quickCount={quickCount} max={maxCount} theme={theme} onChange={(value) => onConfigChange("count", String(value || 1))} />
                        </div>
                    </SettingsSection>
                ) : null}
            </div>
        </ImageSettingsTheme>
    );
}

export function ImageSettingsTheme({ theme, children }: { theme: CanvasTheme; children: ReactNode }) {
    return (
        <ConfigProvider
            theme={{
                token: { colorBgContainer: theme.canvas.background, colorBgElevated: theme.canvas.background, colorBorder: theme.node.stroke, colorPrimary: theme.node.activeStroke, colorText: theme.node.text, colorTextLightSolid: theme.node.panel },
                components: { Button: { defaultBg: theme.canvas.background, defaultBorderColor: theme.node.stroke, defaultColor: theme.node.text } },
            }}
        >
            {children}
        </ConfigProvider>
    );
}

export function imageQualityLabel(value: string) {
    return ({ auto: canvasT("videoCanvas.settings.qualityAuto", "自动"), high: canvasT("videoCanvas.settings.qualityHigh", "高"), medium: canvasT("videoCanvas.settings.qualityMedium", "中"), low: canvasT("videoCanvas.settings.qualityLow", "低") } as Record<string, string>)[value] || canvasT("videoCanvas.settings.qualityDefault", "默认");
}

export function imageSizeLabel(size: string) {
    return aspectOptions.find((item) => (item.size || item.value) === size || item.value === size)?.label || size;
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
