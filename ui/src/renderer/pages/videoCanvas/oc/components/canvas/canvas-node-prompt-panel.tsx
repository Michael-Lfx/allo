import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowUp, Maximize2, Minimize2, Square } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ModelPicker } from "@oc/components/model-picker";
import { configuredModelMatchesCapability, defaultConfig, useEffectiveConfig, type AiConfig } from "@oc/stores/use-config-store";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { getNodeGenerationMode } from "@oc/lib/canvas/node-registry";
import { normalizeVideoDuration, isMiniMaxH3ResolutionToken } from "@oc/lib/video-generation-options";
import { canonicalizeVideoResolution } from "@oc/lib/canvas-video-resolution";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { isMiniMaxH3VideoModel } from "@renderer/services/videoModelCapabilities";
import { CanvasImageSettingsPopover } from "./canvas-image-settings-popover";
import { CanvasAudioSettingsPopover, type CanvasAudioSettingKey } from "./canvas-audio-settings-popover";
import { CanvasChromeButton } from "./canvas-overlay";
import { CanvasResourceMentionTextarea } from "./canvas-resource-mention-textarea";
import { CanvasVideoSettingsPopover, type CanvasVideoSettingKey } from "./canvas-video-settings-popover";
import { CanvasVideoPromptTools } from "./canvas-video-prompt-tools";
import { CanvasPresetPicker, type CanvasPromptPreset } from "./canvas-preset-picker";
import { CanvasPortraitTexturePopover } from "./canvas-portrait-texture-popover";
import { CanvasNodeType, type CanvasGenerationMode, type CanvasNodeData, type CanvasNodeMetadata, type CanvasWorkspaceMode } from "@oc/types/canvas";
import type { CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import { resolveModelVideoBooleanOptions } from "@oc/lib/model-capabilities";

export type CanvasNodeGenerationMode = CanvasGenerationMode;

type CanvasNodePromptPanelProps = {
    node: CanvasNodeData;
    isRunning: boolean;
    onPromptChange: (nodeId: string, prompt: string) => void;
    onConfigChange: (nodeId: string, patch: Partial<CanvasNodeMetadata>) => void;
    onGenerate: (nodeId: string, mode: CanvasNodeGenerationMode, prompt: string) => void;
    onStop: (nodeId: string) => void;
    mentionReferences?: CanvasResourceReference[];
    onImageSettingsOpenChange?: (open: boolean) => void;
    workspaceMode?: CanvasWorkspaceMode;
};

export function CanvasNodePromptPanel({ node, isRunning, onPromptChange, onConfigChange, onGenerate, onStop, mentionReferences = [], onImageSettingsOpenChange }: CanvasNodePromptPanelProps) {
    useTranslation();
    const globalConfig = useEffectiveConfig();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const mode = getNodeGenerationMode(node) ?? "image";
    const config = buildNodeConfig(globalConfig, node, mode);
    const hasTextContent = node.type === CanvasNodeType.Text && Boolean(node.metadata?.content?.trim());
    const hasImageContent = node.type === CanvasNodeType.Image && Boolean(node.metadata?.content);
    const [prompt, setPrompt] = useState(node.metadata?.composerContent ?? node.metadata?.prompt ?? "");
    const [presetOpen, setPresetOpen] = useState(false);
    const [expanded, setExpanded] = useState(false);
    const [promptContentHeight, setPromptContentHeight] = useState(0);
    const videoFrameOptions = mentionReferences.filter((item) => item.active && item.kind === "image").map((item) => ({ nodeId: item.nodeId, label: item.label, title: item.title, previewUrl: item.previewUrl }));
    const hasVideoPromptTools = mode === "video" && videoFrameOptions.length > 0;
    const composerMinHeight = expanded ? 220 : 56;
    const composerHeight = Math.min(expanded ? 360 : 140, Math.max(composerMinHeight, Math.ceil(promptContentHeight + 12)));
    const isSubmitDisabled = !isRunning && !prompt.trim();
    const canExpandPrompt = mode === "image" || mode === "video";
    const isPortraitTexture = mode === "image" && Boolean(node.metadata?.portraitTexture);
    const skillReferences = useMemo(() => mentionReferences.filter((item) => item.kind === "skill"), [mentionReferences]);
    const updatePromptContentHeight = useCallback((height: number) => {
        setPromptContentHeight((current) => (Math.abs(current - height) < 1 ? current : height));
    }, []);

    useEffect(() => {
        setPrompt(node.metadata?.composerContent ?? node.metadata?.prompt ?? "");
    }, [node.id, node.metadata?.composerContent, node.metadata?.prompt]);

    useEffect(() => {
        setPromptContentHeight(0);
        setExpanded(false);
        setPresetOpen(false);
    }, [node.id]);

    const updatePrompt = (value: string) => {
        setPrompt(value);
        onPromptChange(node.id, value);
        if (/(^|\s)\/[\p{L}\p{N}_-]*$/u.test(value)) setPresetOpen(true);
    };

    const applyPreset = (preset: CanvasPromptPreset) => {
        const withoutSlash = prompt.replace(/(^|\s)\/[\p{L}\p{N}_-]*$/u, "$1").trimEnd();
        updatePrompt(withoutSlash ? `${withoutSlash}\n${preset.prompt}` : preset.prompt);
    };

    const submit = () => {
        const text = prompt.trim();
        if (!text || isRunning) return;
        onGenerate(node.id, mode, text);
        setExpanded(false);
    };

    const videoTools = hasVideoPromptTools ? (
        <CanvasVideoPromptTools metadata={node.metadata} frameOptions={videoFrameOptions} onMetadataChange={(patch) => onConfigChange(node.id, patch)} />
    ) : null;

    return (
        <div
            className="canvas-composer-shell canvas-overlay"
            style={canvasOverlayStyle(theme, { borderColor: theme.toolbar.border })}
            onMouseDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            onWheel={(event) => event.stopPropagation()}
        >
            <div className="canvas-composer-field overflow-hidden" style={{ height: composerHeight, background: theme.spatial.surface }}>
                <CanvasResourceMentionTextarea
                    value={prompt}
                    references={mentionReferences}
                    onChange={updatePrompt}
                    containerClassName="min-h-0 h-full"
                    className="thin-scrollbar h-full w-full resize-none overflow-y-auto border-none bg-transparent px-2.5 py-2 text-[var(--fs-body)] leading-5 !outline-none placeholder:text-current placeholder:opacity-35"
                    style={{ color: theme.node.text }}
                    placeholder={promptPlaceholder(mode, hasImageContent, hasTextContent)}
                    onContentSizeChange={updatePromptContentHeight}
                    onSubmit={submit}
                />
            </div>
            {videoTools}

            <div className="canvas-composer-footer">
                {isPortraitTexture ? (
                    <CanvasPortraitTexturePopover value={node.metadata?.portraitTexture} placement="topLeft" onChange={(portraitTexture) => onConfigChange(node.id, { portraitTexture })} />
                ) : (
                    <CanvasPresetPicker mode={mode} skillReferences={skillReferences} open={presetOpen} onOpenChange={setPresetOpen} onSelect={applyPreset} compact />
                )}
                <div className="min-w-0 max-w-[168px] flex-1">
                    <ModelPicker
                        className="!h-7 !w-full !min-w-0 !text-[var(--fs-label)]"
                        fullWidth
                        config={config}
                        value={config.model}
                        onChange={(model) => onConfigChange(node.id, { model })}
                        capability={mode}
                        onMissingConfig={() => navigateToSettings({ continueCreation: true })}
                        showSelectedPrice={false}
                    />
                </div>
                {mode === "text" ? (
                    <input
                        type="number"
                        min={1}
                        max={15}
                        value={Math.max(1, Math.min(15, Math.floor(Math.abs(Number(node.metadata?.textCount) || 1))))}
                        onChange={(event) => onConfigChange(node.id, { textCount: Math.max(1, Math.min(15, Math.floor(Math.abs(Number(event.target.value) || 1)))) })}
                        aria-label={canvasT("videoCanvas.prompt.textCountHint", "文本生成份数（默认 1，可在生成配置中调整）")}
                        className="canvas-chrome-token inline-flex h-7 w-10 px-1 text-center tabular-nums"
                    />
                ) : mode === "image" ? (
                    <CanvasImageSettingsPopover
                        config={config}
                        placement="topLeft"
                        buttonClassName="!max-w-[132px]"
                        onConfigChange={(key, value) => onConfigChange(node.id, key === "count" ? { count: Number(value) || 1 } : { [key]: value })}
                        onOpenChange={onImageSettingsOpenChange}
                    />
                ) : mode === "video" ? (
                    <CanvasVideoSettingsPopover
                        config={config}
                        buttonClassName="!max-w-[120px]"
                        onConfigChange={(key, value) => onConfigChange(node.id, videoConfigPatch(key, value))}
                    />
                ) : mode === "audio" ? (
                    <CanvasAudioSettingsPopover
                        config={config}
                        buttonClassName="!max-w-[108px]"
                        onConfigChange={(key, value) => onConfigChange(node.id, audioConfigPatch(key, value))}
                    />
                ) : null}
                <span className="ml-auto flex shrink-0 items-center gap-1">
                    {canExpandPrompt ? (
                        <CanvasChromeButton
                            className="!size-7 !px-0 justify-center"
                            title={expanded ? canvasT("videoCanvas.toolbar.collapseEdit", "收起") : canvasT("videoCanvas.toolbar.expandEdit", "放大编辑")}
                            aria-label={expanded ? canvasT("videoCanvas.toolbar.collapseEdit", "收起") : canvasT("videoCanvas.toolbar.expandEdit", "放大编辑")}
                            onClick={() => setExpanded((value) => !value)}
                        >
                            {expanded ? <Minimize2 className="size-3" /> : <Maximize2 className="size-3" />}
                        </CanvasChromeButton>
                    ) : null}
                    <button
                        type="button"
                        className="grid size-7 shrink-0 place-items-center rounded-full outline-none disabled:opacity-40"
                        disabled={isSubmitDisabled}
                        style={{
                            background: isSubmitDisabled ? theme.toolbar.itemHover : isRunning ? theme.accent.danger : theme.node.activeStroke,
                            color: isSubmitDisabled ? theme.node.faint : theme.canvas.background,
                        }}
                        onClick={() => (isRunning ? onStop(node.id) : submit())}
                        aria-label={isRunning ? canvasT("videoCanvas.config.stop", "停止") : canvasT("videoCanvas.config.generate", "生成")}
                    >
                        {isRunning ? <Square className="size-2.5 fill-current" /> : <ArrowUp className="size-3" />}
                    </button>
                </span>
            </div>
        </div>
    );
}

function buildNodeConfig(globalConfig: AiConfig, node: CanvasNodeData, mode: CanvasNodeGenerationMode): AiConfig {
    const defaultModel = mode === "image" ? globalConfig.imageModel : mode === "video" ? globalConfig.videoModel : mode === "audio" ? globalConfig.audioModel : globalConfig.textModel;
    const fallbackModel = mode === "image" ? defaultConfig.imageModel : mode === "video" ? defaultConfig.videoModel : mode === "audio" ? defaultConfig.audioModel : defaultConfig.textModel;
    const storedModel = node.metadata?.model;
    const model = storedModel && configuredModelMatchesCapability(globalConfig, storedModel, mode) ? storedModel : defaultModel && configuredModelMatchesCapability(globalConfig, defaultModel, mode) ? defaultModel : fallbackModel;
    const canonical = canonicalizeVideoResolution(model, node.metadata?.vquality || globalConfig.vquality || defaultConfig.vquality);
    const vquality = isMiniMaxH3VideoModel(model) || isMiniMaxH3ResolutionToken(canonical)
        ? canonical
        : String(canonical).replace(/p$/i, "");
    const videoBooleans = resolveModelVideoBooleanOptions(
        globalConfig,
        model,
        { videoGenerateAudio: node.metadata?.generateAudio, videoWatermark: node.metadata?.watermark },
        { videoGenerateAudio: globalConfig.videoGenerateAudio, videoWatermark: globalConfig.videoWatermark },
    );
    return {
        ...globalConfig,
        model,
        quality: node.metadata?.quality || globalConfig.quality || defaultConfig.quality,
        size: node.metadata?.size || globalConfig.size || defaultConfig.size,
        transparentBackground: (node.metadata?.transparentBackground || globalConfig.transparentBackground) === "true" ? "true" : "false",
        videoSeconds: normalizeVideoDuration(node.metadata?.seconds || globalConfig.videoSeconds || defaultConfig.videoSeconds),
        vquality,
        videoGenerateAudio: videoBooleans.videoGenerateAudio,
        videoWatermark: videoBooleans.videoWatermark,
        audioVoice: node.metadata?.audioVoice || globalConfig.audioVoice || defaultConfig.audioVoice,
        audioFormat: node.metadata?.audioFormat || globalConfig.audioFormat || defaultConfig.audioFormat,
        audioSpeed: node.metadata?.audioSpeed || globalConfig.audioSpeed || defaultConfig.audioSpeed,
        audioInstructions: node.metadata?.audioInstructions || globalConfig.audioInstructions || defaultConfig.audioInstructions,
        count: String(node.metadata?.count || (mode === "image" ? globalConfig.canvasImageCount || globalConfig.count : globalConfig.count) || defaultConfig.count),
    };
}

function promptPlaceholder(mode: CanvasNodeGenerationMode, hasImageContent: boolean, hasTextContent: boolean) {
    if (mode === "video") return canvasT("videoCanvas.prompt.placeholderVideo", "描述要生成的视频内容");
    if (mode === "audio") return canvasT("videoCanvas.prompt.placeholderAudio", "描述要生成的音频内容");
    if (mode === "image") return hasImageContent ? canvasT("videoCanvas.prompt.placeholderImageRegen", "输入新提示词，重新生成当前图片") : canvasT("videoCanvas.prompt.placeholderImage", "描述要生成的图片内容");
    return hasTextContent ? canvasT("videoCanvas.prompt.placeholderTextEdit", "请输入你想要将本段文本修改成什么") : canvasT("videoCanvas.prompt.placeholderText", "请输入你想要生成的文本内容");
}

function videoConfigPatch(key: CanvasVideoSettingKey, value: string) {
    if (key === "videoSeconds") return { seconds: value };
    if (key === "videoGenerateAudio") return { generateAudio: value };
    if (key === "videoWatermark") return { watermark: value };
    return { [key]: value };
}

function audioConfigPatch(key: CanvasAudioSettingKey, value: string) {
    if (key === "audioVoice") return { audioVoice: value };
    if (key === "audioFormat") return { audioFormat: value };
    if (key === "audioSpeed") return { audioSpeed: value };
    return { audioInstructions: value };
}
