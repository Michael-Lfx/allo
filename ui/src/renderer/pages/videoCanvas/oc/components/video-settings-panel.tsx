import { useTranslation } from "react-i18next";
import { Switch } from "antd";

import { ImageSettingsTheme } from "@oc/components/image-settings-panel";
import { AspectChoice, ChoiceChip, SettingsPanelHeader, SettingsSection } from "@oc/components/generation-settings-chrome";
import { boolConfig, isSeedanceVideoConfig, normalizeSeedanceDuration, normalizeSeedanceRatio, normalizeSeedanceResolution } from "@oc/lib/seedance-video";
import { normalizeMiniMaxH3Duration } from "@oc/lib/minimax-h3-video";
import { isMiniMaxH3VideoModel, normalizeMiniMaxH3Resolution } from "@renderer/services/videoModelCapabilities";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { type CanvasTheme } from "@oc/lib/canvas-theme";
import { normalizeVideoDuration, normalizeVideoResolution, isVideoResolutionMatch, formatVideoResolutionLabel, videoDimensionsForRatioAndResolution, VIDEO_DURATION_MIN } from "@oc/lib/video-generation-options";
import { modelCapabilityConfigFor, resolveVideoRatioValue, resolveVideoResolutionValue, videoDurationOptions, type VideoCapabilityConfig } from "@oc/lib/model-capabilities";
import { modelOptionName, resolveModelRequestConfig, type AiConfig } from "@oc/stores/use-config-store";
import { CanvasVideoDurationBar } from "./canvas/canvas-video-duration-bar";

function sizeOptions() {
    return [
        { value: "1280x720", label: canvasT("videoCanvas.settings.landscape", "横屏"), width: 1280, height: 720 },
        { value: "720x1280", label: canvasT("videoCanvas.settings.portrait", "竖屏"), width: 720, height: 1280 },
        { value: "1024x1024", label: canvasT("videoCanvas.settings.square", "方形"), width: 1024, height: 1024 },
        { value: "1792x1024", label: canvasT("videoCanvas.settings.widescreen", "宽屏"), width: 1792, height: 1024 },
        { value: "1024x1792", label: canvasT("videoCanvas.settings.tall", "长图"), width: 1024, height: 1792 },
        { value: "auto", label: "auto", width: 0, height: 0 },
    ];
}

type VideoSettingsPanelProps = {
    config: AiConfig;
    onConfigChange: (key: "vquality" | "size" | "videoSeconds" | "videoGenerateAudio" | "videoWatermark", value: string) => void;
    theme: CanvasTheme;
    showTitle?: boolean;
    className?: string;
};

export function VideoSettingsPanel({ config, onConfigChange, theme, showTitle = true, className = "w-[292px] space-y-2.5" }: VideoSettingsPanelProps) {
    useTranslation();
    const profile = modelCapabilityConfigFor(config, config.model).video!;
    if (resolveModelRequestConfig(config, config.model).interfaceType === "volcengine-jimeng-video") {
        return <JiMengVideoSettingsPanel config={config} profile={profile} onConfigChange={onConfigChange} theme={theme} showTitle={showTitle} className={className} />;
    }
    if (isMiniMaxH3VideoModel(modelOptionName(config.model || config.videoModel))) {
        return <MiniMaxH3VideoSettingsPanel config={config} profile={profile} onConfigChange={onConfigChange} theme={theme} showTitle={showTitle} className={className} />;
    }
    if (isSeedanceVideoConfig(config)) {
        return <SeedanceVideoSettingsPanel config={config} profile={profile} onConfigChange={onConfigChange} theme={theme} showTitle={showTitle} className={className} />;
    }

    const seconds = normalizeVideoDuration(config.videoSeconds);
    const secondOptions = videoDurationOptions(profile);
    const resolution = resolveVideoResolutionValue(profile, config.vquality);
    const ratio = resolveVideoRatioValue(profile, config.size);
    const dimensions = videoDimensionsForRatioAndResolution(ratio, resolution);
    const sizeSupported = profile.ratios.length > 0;
    const configuredResolutions = profile.resolutions.map((value) => ({ value: value.replace(/p$/i, ""), label: value.toUpperCase() }));
    const generateAudio = boolConfig(config.videoGenerateAudio, profile.generateAudio.default);
    const watermark = boolConfig(config.videoWatermark, profile.watermark.default);

    return (
        <ImageSettingsTheme theme={theme}>
            <div className={className} style={{ color: theme.node.text }} onMouseDown={(event) => event.stopPropagation()}>
                {showTitle ? <SettingsPanelHeader title={canvasT("videoCanvas.settings.videoTitle", "视频设置")} subtitle={canvasT("videoCanvas.settings.videoSubtitle", "分辨率、画幅与时长会写入这次生成。")} theme={theme} /> : null}
                {configuredResolutions.length ? <SettingsSection title={canvasT("videoCanvas.settings.resolution", "分辨率")} theme={theme}>
                    <div className="grid grid-cols-3 gap-1.5">
                        {configuredResolutions.map((item) => (
                            <ChoiceChip key={item.value} selected={isVideoResolutionMatch(resolution, item.value)} theme={theme} onClick={() => onConfigChange("vquality", item.value)}>
                                {item.label}
                            </ChoiceChip>
                        ))}
                    </div>
                </SettingsSection> : null}
                {sizeSupported ? <SettingsSection title={canvasT("videoCanvas.settings.size", "尺寸")} theme={theme}>
                    {dimensions ? <div className="mb-2 grid grid-cols-[1fr_auto_1fr] items-center gap-1.5">
                        <DimensionValue prefix="W" value={dimensions.width} theme={theme} />
                        <span className="text-xs opacity-45">×</span>
                        <DimensionValue prefix="H" value={dimensions.height} theme={theme} />
                    </div> : null}
                    <div className="grid grid-cols-3 gap-1.5">
                        {profile.ratios.map((value) => (
                            <AspectChoice
                                key={value}
                                selected={ratio === value}
                                label={value}
                                theme={theme}
                                preview={<SizePreview width={ratioPreview(value).width} height={ratioPreview(value).height} color={ratio === value ? theme.node.activeStroke : theme.node.text} />}
                                onClick={() => onConfigChange("size", value)}
                            />
                        ))}
                    </div>
                </SettingsSection> : null}
                <SettingsSection title={canvasT("videoCanvas.settings.seconds", "秒数")} theme={theme}>
                    <CanvasVideoDurationBar
                        value={Number(seconds)}
                        min={profile.duration.min || VIDEO_DURATION_MIN}
                        max={profile.duration.max || Math.max(...secondOptions, profile.duration.min || VIDEO_DURATION_MIN)}
                        step={profile.duration.step || 1}
                        ticks={secondOptions}
                        theme={theme}
                        onChange={(value) => onConfigChange("videoSeconds", String(value))}
                    />
                </SettingsSection>
                {profile.generateAudio.supported || profile.watermark.supported ? <SettingsSection title={canvasT("videoCanvas.settings.output", "输出")} theme={theme}><div className="grid grid-cols-2 gap-3">{profile.generateAudio.supported ? <SwitchRow label={canvasT("videoCanvas.settings.genAudio", "生成声音")} checked={generateAudio} theme={theme} onChange={(checked) => onConfigChange("videoGenerateAudio", String(checked))} /> : null}{profile.watermark.supported ? <SwitchRow label={canvasT("videoCanvas.settings.watermark", "添加水印")} checked={watermark} theme={theme} onChange={(checked) => onConfigChange("videoWatermark", String(checked))} /> : null}</div></SettingsSection> : null}
            </div>
        </ImageSettingsTheme>
    );
}

function MiniMaxH3VideoSettingsPanel({ config, profile, onConfigChange, theme, showTitle, className }: VideoSettingsPanelProps & { profile: VideoCapabilityConfig }) {
    const resolution = normalizeMiniMaxH3Resolution(config.vquality);
    const ratio = profile.ratios.includes(config.size) ? config.size : profile.defaultRatio;
    const duration = normalizeMiniMaxH3Duration(config.videoSeconds);
    const durationOptions = videoDurationOptions(profile);

    return (
        <ImageSettingsTheme theme={theme}>
            <div className={className} style={{ color: theme.node.text }} onMouseDown={(event) => event.stopPropagation()}>
                {showTitle ? <SettingsPanelHeader title={canvasT("videoCanvas.settings.videoTitle", "视频设置")} subtitle={canvasT("videoCanvas.settings.videoSubtitle", "分辨率、画幅与时长会写入这次生成。")} theme={theme} /> : null}
                <SettingsSection title={canvasT("videoCanvas.settings.resolution", "分辨率")} theme={theme}>
                    <div className="grid grid-cols-2 gap-1.5">
                        {profile.resolutions.map((value) => (
                            <ChoiceChip key={value} selected={resolution === value} theme={theme} onClick={() => onConfigChange("vquality", value)}>
                                {value}
                            </ChoiceChip>
                        ))}
                    </div>
                </SettingsSection>
                {profile.ratios.length ? <SettingsSection title={canvasT("videoCanvas.settings.ratio", "比例")} hint={canvasT("videoCanvas.settings.t2vRatioHint", "文生视频需指定比例；图生视频由服务端按画面自适应")} theme={theme}>
                    <div className="grid grid-cols-3 gap-1.5">
                        {profile.ratios.map((value) => (
                            <ChoiceChip key={value} selected={ratio === value} theme={theme} onClick={() => onConfigChange("size", value)}>
                                {value}
                            </ChoiceChip>
                        ))}
                    </div>
                </SettingsSection> : null}
                <SettingsSection title={canvasT("videoCanvas.settings.duration", "时长")} theme={theme}>
                    <CanvasVideoDurationBar
                        value={duration}
                        min={profile.duration.min || VIDEO_DURATION_MIN}
                        max={profile.duration.max || Math.max(...durationOptions, profile.duration.min || VIDEO_DURATION_MIN)}
                        step={profile.duration.step || 1}
                        ticks={durationOptions}
                        theme={theme}
                        onChange={(value) => onConfigChange("videoSeconds", String(value))}
                    />
                </SettingsSection>
            </div>
        </ImageSettingsTheme>
    );
}

function JiMengVideoSettingsPanel({ config, profile, onConfigChange, theme, showTitle, className }: VideoSettingsPanelProps & { profile: VideoCapabilityConfig }) {
    const seconds = normalizeVideoDuration(config.videoSeconds);
    const durationOptions = videoDurationOptions(profile);
    return (
        <ImageSettingsTheme theme={theme}>
            <div className={className} style={{ color: theme.node.text }} onMouseDown={(event) => event.stopPropagation()}>
                {showTitle ? <SettingsPanelHeader title={canvasT("videoCanvas.settings.videoTitle", "视频设置")} subtitle={canvasT("videoCanvas.settings.videoSubtitle", "分辨率、画幅与时长会写入这次生成。")} theme={theme} /> : null}
                <SettingsSection title={canvasT("videoCanvas.settings.ratio", "比例")} theme={theme}>
                    <div className="grid grid-cols-3 gap-1.5">
                {profile.ratios.map((value) => <ChoiceChip key={value} selected={config.size === value} theme={theme} onClick={() => onConfigChange("size", value)}>{value}</ChoiceChip>)}
                    </div>
                </SettingsSection>
                <SettingsSection title={canvasT("videoCanvas.settings.seconds", "秒数")} theme={theme}>
                    <CanvasVideoDurationBar
                        value={Number(seconds)}
                        min={Math.min(...durationOptions)}
                        max={Math.max(...durationOptions)}
                        step={1}
                        ticks={durationOptions}
                        theme={theme}
                        onChange={(value) => onConfigChange("videoSeconds", String(value))}
                    />
                </SettingsSection>
            </div>
        </ImageSettingsTheme>
    );
}

function SeedanceVideoSettingsPanel({ config, profile, onConfigChange, theme, showTitle, className }: VideoSettingsPanelProps & { profile: VideoCapabilityConfig }) {
    const model = modelOptionName(config.model || config.videoModel);
    const resolution = normalizeSeedanceResolution(config.vquality, model);
    const ratio = normalizeSeedanceRatio(config.size);
    const duration = normalizeSeedanceDuration(config.videoSeconds);
    const generateAudio = boolConfig(config.videoGenerateAudio, profile.generateAudio.default);
    const watermark = boolConfig(config.videoWatermark, profile.watermark.default);
    const durationOptions = videoDurationOptions(profile);

    return (
        <ImageSettingsTheme theme={theme}>
            <div className={className} style={{ color: theme.node.text }} onMouseDown={(event) => event.stopPropagation()}>
                {showTitle ? <SettingsPanelHeader title={canvasT("videoCanvas.settings.videoTitle", "视频设置")} subtitle={canvasT("videoCanvas.settings.videoSubtitle", "分辨率、画幅与时长会写入这次生成。")} theme={theme} /> : null}
                <SettingsSection title={canvasT("videoCanvas.settings.resolution", "分辨率")} theme={theme}>
                    <div className="grid grid-cols-3 gap-1.5">
                        {profile.resolutions.map((value) => {
                            const item = { value, label: value.toUpperCase() };
                            return (
                                <ChoiceChip key={item.value} selected={isVideoResolutionMatch(resolution, item.value)} theme={theme} onClick={() => onConfigChange("vquality", item.value)}>
                                    {item.label}
                                </ChoiceChip>
                            );
                        })}
                    </div>
                </SettingsSection>
                <SettingsSection title={canvasT("videoCanvas.settings.ratio", "比例")} theme={theme}>
                    <div className="grid grid-cols-4 gap-1.5">
                        {profile.ratios.map((value) => (
                            <AspectChoice
                                key={value}
                                selected={ratio === value}
                                label={value}
                                theme={theme}
                                preview={<SizePreview width={ratioPreview(value).width} height={ratioPreview(value).height} color={ratio === value ? theme.node.activeStroke : theme.node.text} />}
                                onClick={() => onConfigChange("size", value)}
                            />
                        ))}
                    </div>
                </SettingsSection>
                <SettingsSection title={canvasT("videoCanvas.settings.duration", "时长")} theme={theme}>
                    <CanvasVideoDurationBar
                        value={duration}
                        min={profile.duration.min || VIDEO_DURATION_MIN}
                        max={profile.duration.max || Math.max(...durationOptions, profile.duration.min || VIDEO_DURATION_MIN)}
                        step={profile.duration.step || 1}
                        ticks={durationOptions}
                        theme={theme}
                        onChange={(value) => onConfigChange("videoSeconds", String(value))}
                    />
                </SettingsSection>
                <SettingsSection title={canvasT("videoCanvas.settings.output", "输出")} theme={theme}>
                    <div className="grid grid-cols-2 gap-3">
                        {profile.generateAudio.supported ? <SwitchRow label={canvasT("videoCanvas.settings.genAudio", "生成声音")} checked={generateAudio} theme={theme} onChange={(checked) => onConfigChange("videoGenerateAudio", String(checked))} /> : null}
                        {profile.watermark.supported ? <SwitchRow label={canvasT("videoCanvas.settings.watermark", "添加水印")} checked={watermark} theme={theme} onChange={(checked) => onConfigChange("videoWatermark", String(checked))} /> : null}
                    </div>
                </SettingsSection>
            </div>
        </ImageSettingsTheme>
    );
}

export function videoResolutionLabel(value: string) {
    return formatVideoResolutionLabel(value) || `${normalizeVideoResolutionValue(value)}P`;
}

export function videoSizeLabel(value: string) {
    const ratio = normalizeSeedanceRatio(value);
    if (value === "adaptive" || value === "auto") return canvasT("videoCanvas.settings.adaptive", "自适应");
    if (ratio === value) {
        const ratioLabels: Record<string, string> = {
            "16:9": canvasT("videoCanvas.settings.landscape", "横屏"),
            "9:16": canvasT("videoCanvas.settings.portrait", "竖屏"),
            "1:1": canvasT("videoCanvas.settings.square", "方形"),
            "4:3": canvasT("videoCanvas.settings.standardLandscape", "标准横屏"),
            "3:4": canvasT("videoCanvas.settings.standardPortrait", "标准竖屏"),
            "21:9": canvasT("videoCanvas.settings.ultrawide", "宽银幕"),
            adaptive: canvasT("videoCanvas.settings.adaptive", "自适应"),
        };
        return ratioLabels[ratio] || ratio;
    }
    const size = normalizeVideoSizeValue(value);
    return sizeOptions().find((item) => item.value === size)?.label || size;
}

export function videoSecondsLabel(value: string) {
    return `${normalizeVideoDuration(value)}s`;
}

export function normalizeVideoSizeValue(value: string) {
    if (value === "auto") return "auto";
    if (/^\d+x\d+$/.test(value || "")) return value;
    return ["9:16", "2:3", "3:4"].includes(value) ? "720x1280" : "1280x720";
}

export function normalizeVideoResolutionValue(value: string) {
    return normalizeVideoResolution(value);
}

function DimensionValue({ prefix, value, theme }: { prefix: string; value: number; theme: CanvasTheme }) {
    return (
        <div className="flex h-8 overflow-hidden rounded-lg border text-[var(--fs-label)]" style={{ background: theme.canvas.background, borderColor: theme.node.stroke, color: theme.node.text }}>
            <span className="grid w-7 place-items-center" style={{ color: theme.node.muted }}>
                {prefix}
            </span>
            <span className="min-w-0 flex-1 px-2 leading-8 tabular-nums">{value}</span>
        </div>
    );
}

function SizePreview({ width, height, color }: { width: number; height: number; color: string }) {
    if (!width || !height) return null;
    const longSide = Math.max(width, height);
    const previewWidth = Math.max(7, Math.round((width / longSide) * 16));
    const previewHeight = Math.max(7, Math.round((height / longSide) * 16));
    return <span className="shrink-0 rounded-[2px] border" style={{ width: previewWidth, height: previewHeight, borderColor: color }} />;
}

function ratioPreview(ratio: string) {
    if (ratio === "9:16") return { width: 9, height: 16 };
    if (ratio === "1:1") return { width: 1, height: 1 };
    if (ratio === "4:3") return { width: 4, height: 3 };
    if (ratio === "3:4") return { width: 3, height: 4 };
    if (ratio === "21:9") return { width: 21, height: 9 };
    if (ratio === "adaptive") return { width: 0, height: 0 };
    return { width: 16, height: 9 };
}

function SwitchRow({ label, checked, theme, onChange }: { label: string; checked: boolean; theme: CanvasTheme; onChange: (checked: boolean) => void }) {
    return (
        <div className="flex h-8 items-center justify-between gap-2">
            <span className="min-w-0 whitespace-nowrap text-[var(--fs-label)]" style={{ color: theme.node.text }}>
                {label}
            </span>
            <span className="shrink-0" onMouseDown={(event) => event.stopPropagation()}>
                <Switch size="small" checked={checked} onChange={onChange} />
            </span>
        </div>
    );
}
