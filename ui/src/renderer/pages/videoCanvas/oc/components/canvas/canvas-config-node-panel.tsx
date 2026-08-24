import type { CSSProperties } from "react";
import { Image as ImageIcon, LoaderCircle, MessageSquare, Music2, Play, Settings2, Square, Video } from "lucide-react";
import { Button, InputNumber, Segmented, Select } from "antd";
import { useTranslation } from "react-i18next";

import { ModelPicker } from "@oc/components/model-picker";
import { configuredModelMatchesCapability, defaultConfig, useEffectiveConfig, type AiConfig } from "@oc/stores/use-config-store";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { normalizeVideoDuration, normalizeVideoResolution } from "@oc/lib/video-generation-options";
import { modelCapabilityConfigFor, normalizeVideoValue, videoDurationAllowed } from "@oc/lib/model-capabilities";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasImageSettingsPopover } from "./canvas-image-settings-popover";
import { CanvasAudioSettingsPopover, type CanvasAudioSettingKey } from "./canvas-audio-settings-popover";
import { CanvasVideoSettingsPopover } from "./canvas-video-settings-popover";
import type { CanvasGenerationMode, CanvasNodeData, CanvasNodeMetadata, CanvasVideoEditOperation, CanvasWorkspaceMode } from "@oc/types/canvas";

type CanvasConfigNodePanelProps = {
    node: CanvasNodeData;
    isRunning: boolean;
    inputSummary: { textCount: number; imageCount: number; videoCount: number; audioCount: number };
    onConfigChange: (nodeId: string, patch: Partial<CanvasNodeMetadata>) => void;
    onGenerate: (nodeId: string) => void;
    onStop: (nodeId: string) => void;
    onComposerToggle: () => void;
    workspaceMode?: CanvasWorkspaceMode;
};

function videoOperationOptions(): Array<{ label: string; value: CanvasVideoEditOperation }> {
    return [
        { label: canvasT("videoCanvas.config.opTextToVideo", "文生视频"), value: "text_to_video" },
        { label: canvasT("videoCanvas.config.opImageToVideo", "图生视频"), value: "image_to_video" },
        { label: canvasT("videoCanvas.config.opExtend", "视频续写"), value: "extend" },
        { label: canvasT("videoCanvas.config.opInpaint", "局部修改"), value: "inpaint" },
        { label: canvasT("videoCanvas.config.opReplace", "元素替换"), value: "replace_element" },
        { label: canvasT("videoCanvas.config.opCamera", "运镜调整"), value: "camera_motion" },
        { label: canvasT("videoCanvas.config.opStyle", "风格迁移"), value: "style_transfer" },
        { label: canvasT("videoCanvas.config.opAudioToVideo", "音频生视频"), value: "audio_to_video" },
        { label: canvasT("videoCanvas.config.opCompare", "版本对比"), value: "compare_versions" },
    ];
}

export function CanvasConfigNodePanel({ node, isRunning, inputSummary, onConfigChange, onGenerate, onStop, onComposerToggle, workspaceMode = "professional" }: CanvasConfigNodePanelProps) {
    useTranslation();
    const globalConfig = useEffectiveConfig();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const mode = node.metadata?.generationMode || "image";
    const simpleMode = workspaceMode === "simple";
    const config = buildNodeConfig(globalConfig, node, mode);
    const videoProfile = mode === "video" ? modelCapabilityConfigFor(config, config.model).video! : undefined;
    const allOps = videoOperationOptions();
    const operationOptions = videoProfile ? allOps.filter((item) => videoProfile.operations.includes(item.value) || item.value === "concat") : allOps;
    const textCountValue = Math.max(1, Math.min(15, Math.floor(Math.abs(Number(node.metadata?.textCount) || 1))));
    const chipStyle = { background: theme.node.fill, borderColor: theme.node.stroke, color: theme.node.text };
    const hasAnyInput = Boolean(inputSummary.textCount || inputSummary.imageCount || inputSummary.videoCount || inputSummary.audioCount);
    const hasComposerContent = Boolean((node.metadata?.composerContent ?? node.metadata?.prompt ?? "").trim());
    const capabilityError = videoProfile ? videoCapabilityError(videoProfile, config.videoSeconds, node.metadata?.composerContent ?? node.metadata?.prompt ?? "", inputSummary, node.metadata?.videoEditOperation) : "";
    const canGenerate = (hasComposerContent || (mode === "audio" ? inputSummary.textCount > 0 : hasAnyInput)) && !capabilityError;

    return (
        <div className="flex h-full w-full cursor-move flex-col px-3 pb-3 pt-7 text-sm" style={{ color: theme.node.text }} onWheel={(event) => event.stopPropagation()}>
            <div className="mb-2 flex items-center justify-between gap-3">
                <div className="shrink-0 text-sm font-semibold">{simpleMode ? canvasT("videoCanvas.config.quickGenerate", "快速生成") : canvasT("videoCanvas.config.genConfig", "生成配置")}</div>
                {simpleMode ? <span className="rounded-md px-2 py-1 text-[var(--fs-tiny)]" style={{ background: theme.node.fill, color: theme.node.muted }}>{canvasT("videoCanvas.config.autoConfig", "自动配置")}</span> : <div className="cursor-default" onMouseDown={(event) => event.stopPropagation()}>
                    <Segmented
                        size="small"
                        className="canvas-config-mode !rounded-md !p-0.5"
                        value={mode}
                        onChange={(value) => onConfigChange(node.id, { generationMode: value as CanvasGenerationMode })}
                        options={[
                            {
                                value: "image",
                                label: (
                                    <span className="inline-flex items-center gap-1">
                                        <ImageIcon className="size-3.5" />
                                        {canvasT("videoCanvas.config.modeImage", "生图")}
                                    </span>
                                ),
                            },
                            {
                                value: "text",
                                label: (
                                    <span className="inline-flex items-center gap-1">
                                        <MessageSquare className="size-3.5" />
                                        {canvasT("videoCanvas.config.modeText", "文本")}
                                    </span>
                                ),
                            },
                            {
                                value: "video",
                                label: (
                                    <span className="inline-flex items-center gap-1">
                                        <Video className="size-3.5" />
                                        {canvasT("videoCanvas.config.modeVideo", "视频")}
                                    </span>
                                ),
                            },
                            {
                                value: "audio",
                                label: (
                                    <span className="inline-flex items-center gap-1">
                                        <Music2 className="size-3.5" />
                                        {canvasT("videoCanvas.config.modeAudio", "音频")}
                                    </span>
                                ),
                            },
                        ]}
                    />
                </div>}
            </div>

            <div className="mb-2 flex flex-wrap gap-1.5">
                <InputChip label={canvasT("videoCanvas.config.prompt", "提示词")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.textCount })} style={chipStyle} />
                <InputChip label={canvasT("videoCanvas.config.refImage", "参考图")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.imageCount })} style={chipStyle} />
                <InputChip label={canvasT("videoCanvas.config.refVideo", "参考视频")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.videoCount })} style={chipStyle} />
                <InputChip label={canvasT("videoCanvas.config.refAudio", "参考音频")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.audioCount })} style={chipStyle} />
                <button type="button" className="inline-flex h-7 cursor-pointer items-center gap-1 rounded-md border px-2 text-[var(--fs-label)]" style={chipStyle} onMouseDown={(event) => event.stopPropagation()} onClick={onComposerToggle}>
                    {simpleMode ? <MessageSquare className="size-3.5" /> : <Settings2 className="size-3.5" />}
                    {simpleMode ? "编辑生成内容" : "组装提示词"}
                </button>
            </div>

            {mode === "video" && !simpleMode ? (
                <div className="mb-2 cursor-default" data-canvas-no-zoom onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
                    <Select
                        size="small"
                        className="canvas-compact-control canvas-control-select !h-9 !w-full"
                        value={node.metadata?.videoEditOperation || defaultVideoOperation(inputSummary)}
                        options={operationOptions}
                        placement="bottomLeft"
                        popupMatchSelectWidth={false}
                        styles={{ popup: { root: { minWidth: 180, maxWidth: 260 } } }}
                        popupRender={(menu) => (
                            <div data-canvas-no-zoom onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
                                {menu}
                            </div>
                        )}
                        onChange={(value) => onConfigChange(node.id, { videoEditOperation: value })}
                    />
                </div>
            ) : null}

            <div className={`mb-2 grid min-w-0 cursor-default items-center gap-2 ${mode === "image" || mode === "video" || mode === "audio" || mode === "text" ? "grid-cols-[minmax(0,1fr)_148px]" : "grid-cols-1"}`} onMouseDown={(event) => event.stopPropagation()}>
                    <ModelPicker className="canvas-compact-control h-10" config={config} value={config.model} onChange={(model) => onConfigChange(node.id, { model })} capability={mode} onMissingConfig={() => navigateToSettings({ continueCreation: true })} fullWidth showSelectedPrice={false} />
                    {mode === "text" ? (
                        <div className="flex h-10 min-w-0 cursor-default items-center justify-between gap-2 rounded-lg border px-2.5" style={{ borderColor: theme.node.stroke, background: theme.node.fill }} data-canvas-no-zoom onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
                            <span className="inline-flex items-center gap-1 text-[var(--fs-tiny)] font-semibold" style={{ color: theme.node.muted }}><MessageSquare className="size-3.5" />{canvasT("videoCanvas.config.textCount", "文本份数")}</span>
                            <InputNumber size="small" min={1} max={15} value={textCountValue} onChange={(value) => onConfigChange(node.id, { textCount: Math.max(1, Math.min(15, Math.floor(Math.abs(Number(value)) || 1))) })} aria-label={canvasT("videoCanvas.prompt.textCountHint", "文本生成份数（默认 1，可在生成配置中调整）")} />
                        </div>
                    ) : mode === "video" ? (
                        <CanvasVideoSettingsPopover config={config} placement="topRight" buttonClassName="canvas-compact-control !h-10 !w-full !justify-start !rounded-lg !px-2" onConfigChange={(key, value) => onConfigChange(node.id, videoConfigPatch(key, value))} />
                    ) : mode === "image" ? (
                        <CanvasImageSettingsPopover config={config} placement="topRight" autoAdjustOverflow={false} buttonClassName="canvas-compact-control !h-10 !w-full !justify-start !rounded-lg !px-2" onConfigChange={(key, value) => onConfigChange(node.id, key === "count" ? { count: Number(value) || 1 } : { [key]: value })} />
                    ) : mode === "audio" ? (
                        <CanvasAudioSettingsPopover config={config} placement="topRight" buttonClassName="canvas-compact-control !h-10 !w-full !justify-start !rounded-lg !px-2" onConfigChange={(key, value) => onConfigChange(node.id, audioConfigPatch(key, value))} />
                    ) : null}
                </div>

            {capabilityError ? <div className="mb-2 rounded-md px-2 py-1.5 text-[var(--fs-tiny)]" style={{ background: theme.accent.danger + "18", color: theme.accent.danger }}>{capabilityError}</div> : null}

            <Button
                type="primary"
                className="mt-auto !h-9 !w-full !cursor-pointer !rounded-lg"
                danger={isRunning}
                disabled={!isRunning && !canGenerate}
                onMouseDown={(event) => event.stopPropagation()}
                onClick={() => (isRunning ? onStop(node.id) : onGenerate(node.id))}
            >
                <span className="inline-flex items-center gap-1.5">
                    {isRunning ? (
                        <>
                            <LoaderCircle className="size-4 animate-spin" />
                            <Square className="size-3.5 fill-current" />
                            <span>{canvasT("videoCanvas.config.stop", "停止")}</span>
                        </>
                    ) : (
                        <>
                            <span>{canvasT("videoCanvas.config.generate", "生成")}</span>
                            <Play className="size-4" />
                            <span>{canvasT("videoCanvas.config.startGenerate", "开始生成")}</span>
                        </>
                    )}
                </span>
            </Button>
        </div>
    );
}

function defaultVideoOperation(inputSummary: CanvasConfigNodePanelProps["inputSummary"]): CanvasVideoEditOperation {
    if (inputSummary.audioCount > 0 && inputSummary.imageCount === 0 && inputSummary.videoCount === 0) return "audio_to_video";
    if (inputSummary.videoCount > 0) return "extend";
    if (inputSummary.imageCount > 0) return "image_to_video";
    return "image_to_video";
}

function InputChip({ label, value, style }: { label: string; value: string; style: CSSProperties }) {
    return (
        <div className="inline-flex h-7 items-center gap-1 rounded-md border px-2 text-[var(--fs-label)]" style={style}>
            <span>{label}</span>
            <span className="font-medium">{value}</span>
        </div>
    );
}

function buildNodeConfig(globalConfig: AiConfig, node: CanvasNodeData, mode: CanvasGenerationMode): AiConfig {
    const defaultModel = mode === "image" ? globalConfig.imageModel : mode === "video" ? globalConfig.videoModel : mode === "audio" ? globalConfig.audioModel : globalConfig.textModel;
    const fallbackModel = mode === "image" ? defaultConfig.imageModel : mode === "video" ? defaultConfig.videoModel : mode === "audio" ? defaultConfig.audioModel : defaultConfig.textModel;
    const storedModel = node.metadata?.model;
    const model = storedModel && configuredModelMatchesCapability(globalConfig, storedModel, mode) ? storedModel : defaultModel && configuredModelMatchesCapability(globalConfig, defaultModel, mode) ? defaultModel : fallbackModel;
    const videoProfile = mode === "video" ? modelCapabilityConfigFor(globalConfig, model).video! : undefined;
    const normalizedVideo = videoProfile ? normalizeVideoValue(videoProfile, { seconds: node.metadata?.seconds || globalConfig.videoSeconds || defaultConfig.videoSeconds, ratio: node.metadata?.size || globalConfig.size || defaultConfig.size, resolution: node.metadata?.vquality || globalConfig.vquality || defaultConfig.vquality }) : undefined;
    return {
        ...globalConfig,
        model,
        quality: node.metadata?.quality || globalConfig.quality || defaultConfig.quality,
        size: normalizedVideo?.ratio || node.metadata?.size || globalConfig.size || defaultConfig.size,
        transparentBackground: (node.metadata?.transparentBackground || globalConfig.transparentBackground) === "true" ? "true" : "false",
        videoSeconds: normalizedVideo?.seconds || normalizeVideoDuration(node.metadata?.seconds || globalConfig.videoSeconds || defaultConfig.videoSeconds),
        vquality: normalizedVideo?.resolution.replace(/p$/i, "") || normalizeVideoResolution(node.metadata?.vquality || globalConfig.vquality || defaultConfig.vquality),
        videoGenerateAudio: videoProfile?.generateAudio.supported ? node.metadata?.generateAudio || globalConfig.videoGenerateAudio || String(videoProfile.generateAudio.default) : "false",
        videoWatermark: videoProfile?.watermark.supported ? node.metadata?.watermark || globalConfig.videoWatermark || String(videoProfile.watermark.default) : "false",
        audioVoice: node.metadata?.audioVoice || globalConfig.audioVoice || defaultConfig.audioVoice,
        audioFormat: node.metadata?.audioFormat || globalConfig.audioFormat || defaultConfig.audioFormat,
        audioSpeed: node.metadata?.audioSpeed || globalConfig.audioSpeed || defaultConfig.audioSpeed,
        audioInstructions: node.metadata?.audioInstructions || globalConfig.audioInstructions || defaultConfig.audioInstructions,
        count: String(node.metadata?.count || (mode === "image" ? globalConfig.canvasImageCount || globalConfig.count : globalConfig.count) || defaultConfig.count),
    };
}

function videoConfigPatch(key: keyof AiConfig, value: string) {
    if (key === "videoSeconds") return { seconds: value };
    if (key === "videoGenerateAudio") return { generateAudio: value };
    if (key === "videoWatermark") return { watermark: value };
    return { [key]: value };
}

function videoCapabilityError(profile: NonNullable<ReturnType<typeof modelCapabilityConfigFor>["video"]>, seconds: string, prompt: string, input: CanvasConfigNodePanelProps["inputSummary"], operation?: string) {
    if (!videoDurationAllowed(profile, Number(seconds))) return "当前模型不支持该视频时长";
    if (Array.from(prompt).length > profile.references.promptMaxChars) return `提示词超过模型限制（最多 ${profile.references.promptMaxChars} 字）`;
    if (input.imageCount > profile.references.maxImages || input.videoCount > profile.references.maxVideos || input.audioCount > profile.references.maxAudios) return "参考素材数量超过当前模型限制";
    const resolvedOperation = operation || (input.audioCount > 0 && input.imageCount === 0 && input.videoCount === 0 ? "audio_to_video" : input.videoCount > 0 ? "extend" : input.imageCount > 0 ? "image_to_video" : "text_to_video");
    if (!profile.operations.includes(resolvedOperation)) return "当前模型不支持该生成模式";
    return "";
}

function audioConfigPatch(key: CanvasAudioSettingKey, value: string) {
    if (key === "audioVoice") return { audioVoice: value };
    if (key === "audioFormat") return { audioFormat: value };
    if (key === "audioSpeed") return { audioSpeed: value };
    return { audioInstructions: value };
}
