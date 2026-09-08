import type { CSSProperties } from "react";
import { Image as ImageIcon, LoaderCircle, MessageSquare, Music2, Play, Settings2, Square, Video } from "lucide-react";
import { Button, InputNumber, Segmented, Select } from "antd";
import { useTranslation } from "react-i18next";

import { ModelPicker } from "@oc/components/model-picker";
import { useEffectiveConfig } from "@oc/stores/use-config-store";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { canvasModelSpecPatch, modelCapabilityConfigFor, videoDurationAllowed } from "@oc/lib/model-capabilities";
import { buildGenerationConfig } from "@oc/lib/canvas/canvas-project-generation";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasImageSettingsPopover } from "./canvas-image-settings-popover";
import { CanvasAudioSettingsPopover, type CanvasAudioSettingKey } from "./canvas-audio-settings-popover";
import { CanvasVideoSettingsPopover, type CanvasVideoSettingKey } from "./canvas-video-settings-popover";
import { defaultVideoOperation } from "@oc/lib/canvas/canvas-config-defaults";
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
        { label: canvasT("videoCanvas.config.opReferenceToVideo", "多图参考视频"), value: "reference_to_video" },
        { label: canvasT("videoCanvas.config.opExtend", "视频续写"), value: "extend" },
        { label: canvasT("videoCanvas.config.opInpaint", "局部修改"), value: "inpaint" },
        { label: canvasT("videoCanvas.config.opReplace", "元素替换"), value: "replace_element" },
        { label: canvasT("videoCanvas.config.opCamera", "运镜调整"), value: "camera_motion" },
        { label: canvasT("videoCanvas.config.opStyle", "风格迁移"), value: "style_transfer" },
        { label: canvasT("videoCanvas.config.opAudioToVideo", "音频生视频"), value: "audio_to_video" },
        { label: canvasT("videoCanvas.config.opCompare", "版本对比"), value: "compare_versions" },
    ];
}

export function CanvasConfigNodePanel({ node, isRunning, inputSummary, onConfigChange, onGenerate, onStop, onComposerToggle }: CanvasConfigNodePanelProps) {
    useTranslation();
    const globalConfig = useEffectiveConfig();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const mode = node.metadata?.generationMode || "image";
    const config = buildGenerationConfig(globalConfig, node, mode);
    const videoProfile = mode === "video" ? modelCapabilityConfigFor(config, config.model).video! : undefined;
    const allOps = videoOperationOptions();
    const operationOptions = videoProfile ? allOps.filter((item) => videoProfile.operations.includes(item.value)) : allOps;
    const textCountValue = Math.max(1, Math.min(15, Math.floor(Math.abs(Number(node.metadata?.textCount) || 1))));
    const chipStyle = { background: theme.node.fill, borderColor: theme.node.stroke, color: theme.node.text };
    const hasAnyInput = Boolean(inputSummary.textCount || inputSummary.imageCount || inputSummary.videoCount || inputSummary.audioCount);
    const hasComposerContent = Boolean((node.metadata?.composerContent ?? node.metadata?.prompt ?? "").trim());
    const capabilityError = videoProfile ? videoCapabilityError(videoProfile, config.videoSeconds, node.metadata?.composerContent ?? node.metadata?.prompt ?? "", inputSummary, node.metadata?.videoEditOperation) : "";
    const canGenerate = (hasComposerContent || (mode === "audio" ? inputSummary.textCount > 0 : hasAnyInput)) && !capabilityError;
    const modeTitle = mode === "video"
        ? canvasT("videoCanvas.config.genConfigVideo", "视频生成配置")
        : mode === "audio"
            ? canvasT("videoCanvas.config.genConfigAudio", "音频生成配置")
            : mode === "text"
                ? canvasT("videoCanvas.config.genConfigText", "文本生成配置")
                : canvasT("videoCanvas.config.genConfigImage", "图片生成配置");

    return (
        <div className="flex h-full w-full cursor-move flex-col px-3 pb-3 pt-7 text-sm" style={{ color: theme.node.text }} onWheel={(event) => event.stopPropagation()}>
            <div className="mb-2">
                <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0 truncate text-sm font-semibold">{modeTitle}</div>
                    <div className="cursor-default" onMouseDown={(event) => event.stopPropagation()}>
                    <Segmented
                        size="small"
                        className="canvas-config-mode !rounded-md !p-0.5"
                        value={mode}
                        onChange={(value) => {
                            const generationMode = value as CanvasGenerationMode;
                            const next = buildGenerationConfig(globalConfig, { ...node, metadata: { ...node.metadata, generationMode } }, generationMode);
                            onConfigChange(node.id, {
                                generationMode,
                                model: next.model,
                                ...canvasModelSpecPatch(globalConfig, next.model, generationMode, mediaSpecValues(node)),
                            });
                        }}
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
                    </div>
                </div>
                <p className="mt-1.5 text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>
                    {hasAnyInput || hasComposerContent
                        ? canvasT("videoCanvas.config.howTo", "把参考素材连到左侧，在这里写提示词并选择模型，生成结果会出现在旁边。")
                        : canvasT("videoCanvas.config.howToEmpty", "先把图、视频或文本连到左侧，或点「组装提示词」用 @ 引用画布素材。")}
                </p>
            </div>

            <div className="mb-2 flex flex-wrap gap-1.5">
                <InputChip label={canvasT("videoCanvas.config.prompt", "提示词")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.textCount })} style={chipStyle} />
                <InputChip label={canvasT("videoCanvas.config.refImage", "参考图")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.imageCount })} style={chipStyle} />
                <InputChip label={canvasT("videoCanvas.config.refVideo", "参考视频")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.videoCount })} style={chipStyle} />
                <InputChip label={canvasT("videoCanvas.config.refAudio", "参考音频")} value={canvasT("videoCanvas.config.countUnit", "{{count}} 个", { count: inputSummary.audioCount })} style={chipStyle} />
                <button type="button" className="inline-flex h-7 cursor-pointer items-center gap-1 rounded-md border px-2 text-[var(--fs-label)]" style={chipStyle} onMouseDown={(event) => event.stopPropagation()} onClick={onComposerToggle}>
                    <Settings2 className="size-3.5" />
                    {canvasT("videoCanvas.config.assemblePrompt", "组装提示词")}
                </button>
            </div>

            {mode === "video" ? (
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
                    <ModelPicker className="canvas-compact-control h-10" config={config} value={config.model} onChange={(model) => onConfigChange(node.id, { model, ...canvasModelSpecPatch(globalConfig, model, mode, mediaSpecValues(node)) })} capability={mode} onMissingConfig={() => navigateToSettings({ continueCreation: true })} fullWidth showSelectedPrice={false} />
                    {mode === "text" ? (
                        <div className="flex h-10 min-w-0 cursor-default items-center justify-between gap-2 rounded-lg border px-2.5" style={{ borderColor: theme.node.stroke, background: theme.node.fill }} data-canvas-no-zoom onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
                            <span className="inline-flex items-center gap-1 text-[var(--fs-tiny)] font-semibold" style={{ color: theme.node.muted }}><MessageSquare className="size-3.5" />{canvasT("videoCanvas.config.textCount", "文本份数")}</span>
                            <InputNumber size="small" min={1} max={15} value={textCountValue} onChange={(value) => onConfigChange(node.id, { textCount: Math.max(1, Math.min(15, Math.floor(Math.abs(Number(value)) || 1))) })} aria-label={canvasT("videoCanvas.prompt.textCountHint", "文本生成份数（默认 1，可在生成配置中调整）")} />
                        </div>
                    ) : mode === "video" ? (
                        <CanvasVideoSettingsPopover config={config} placement="topRight" buttonClassName="canvas-compact-control !h-10 !w-full !justify-start !rounded-lg !px-2" onConfigChange={(key, value) => onConfigChange(node.id, videoConfigPatch(key, value))} />
                    ) : mode === "image" ? (
                        <CanvasImageSettingsPopover config={config} placement="topRight" buttonClassName="canvas-compact-control !h-10 !w-full !justify-start !rounded-lg !px-2" onConfigChange={(key, value) => onConfigChange(node.id, key === "count" ? { count: Number(value) || 1 } : { [key]: value })} />
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
                            <Play className="size-4" />
                            <span>{canvasT("videoCanvas.config.generate", "生成")}</span>
                        </>
                    )}
                </span>
            </Button>
        </div>
    );
}

function InputChip({ label, value, style }: { label: string; value: string; style: CSSProperties }) {
    return (
        <div className="inline-flex h-7 items-center gap-1 rounded-md border px-2 text-[var(--fs-label)]" style={style}>
            <span>{label}</span>
            <span className="font-medium">{value}</span>
        </div>
    );
}

function mediaSpecValues(node: CanvasNodeData) {
    return {
        size: node.metadata?.size,
        quality: node.metadata?.quality,
        seconds: node.metadata?.seconds,
        vquality: node.metadata?.vquality,
        generateAudio: node.metadata?.generateAudio,
        watermark: node.metadata?.watermark,
        transparentBackground: node.metadata?.transparentBackground,
    };
}

function videoConfigPatch(key: CanvasVideoSettingKey, value: string) {
    if (key === "videoSeconds") return { seconds: value };
    if (key === "videoGenerateAudio") return { generateAudio: value };
    if (key === "videoWatermark") return { watermark: value };
    return { [key]: value };
}

function videoCapabilityError(profile: NonNullable<ReturnType<typeof modelCapabilityConfigFor>["video"]>, seconds: string, prompt: string, input: CanvasConfigNodePanelProps["inputSummary"], operation?: string) {
    if (!videoDurationAllowed(profile, Number(seconds))) return canvasT("videoCanvas.config.durationUnsupported", "当前模型不支持该视频时长");
    if (Array.from(prompt).length > profile.references.promptMaxChars) return canvasT("videoCanvas.config.promptTooLong", "提示词超过模型限制（最多 {{count}} 字）", { count: profile.references.promptMaxChars });
    if (input.imageCount > profile.references.maxImages || input.videoCount > profile.references.maxVideos || input.audioCount > profile.references.maxAudios) return canvasT("videoCanvas.config.refsExceeded", "参考素材数量超过当前模型限制");
    const resolvedOperation = operation || defaultVideoOperation(input);
    if (!profile.operations.includes(resolvedOperation)) return canvasT("videoCanvas.config.operationUnsupported", "当前模型不支持该生成模式");
    return "";
}

function audioConfigPatch(key: CanvasAudioSettingKey, value: string) {
    if (key === "audioVoice") return { audioVoice: value };
    if (key === "audioFormat") return { audioFormat: value };
    if (key === "audioSpeed") return { audioSpeed: value };
    return { audioInstructions: value };
}
