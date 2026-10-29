import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { LayoutTemplate, Maximize2, Minimize2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ModelPicker } from "@oc/components/model-picker";
import { useEffectiveConfig } from "@oc/stores/use-config-store";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes, type CanvasTheme } from "@oc/lib/canvas-theme";
import { getOcPortalHost } from "@oc/lib/oc-scope";
import { getNodeGenerationMode } from "@oc/lib/canvas/node-registry";
import { canvasModelSpecPatch } from "@oc/lib/model-capabilities";
import { buildGenerationConfig } from "@oc/lib/canvas/canvas-project-generation";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { useCanvasColorTheme } from "@oc/stores/use-canvas-color-theme";
import { CanvasCameraPicker } from "./canvas-camera-picker";
import { CanvasImageSettingsPopover } from "./canvas-image-settings-popover";
import { CanvasAudioSettingsPopover, type CanvasAudioSettingKey } from "./canvas-audio-settings-popover";
import { CanvasChromeButton } from "./canvas-overlay";
import { CanvasResourceMentionTextarea } from "./canvas-resource-mention-textarea";
import { CanvasVideoSettingsPopover, type CanvasVideoSettingKey } from "./canvas-video-settings-popover";
import { CanvasVideoPromptTools } from "./canvas-video-prompt-tools";
import { listedRecipeIds, removeRecipeToken, stripRecipeTokens } from "@oc/lib/canvas/craft/tokens";
import { mergeCraftAttachments } from "@oc/lib/canvas/craft/catalog";
import { listedSkillIds, removeSkillToken, stripSkillTokens } from "@oc/lib/canvas/canvas-skill-mentions";
import { readCameraRig } from "@oc/lib/canvas/canvas-camera-rig";
import { isCanvasPromptOptimizeEnabled } from "@oc/lib/canvas/canvas-generation-enhance";
import { CanvasCraftTokenChip, recipeAttachmentChip, skillAttachmentChip } from "./canvas-craft-token-chip";
import { CanvasComposerPill } from "./canvas-composer-pill";
import { CanvasComposerSendButton } from "./canvas-composer-send-button";
import { CanvasPresetPicker, type CanvasPromptPreset } from "./canvas-preset-picker";
import { CanvasTemplateSlotBar } from "./canvas-template-slot-bar";
import { CanvasPortraitTexturePopover } from "./canvas-portrait-texture-popover";
import { CanvasNodeType, type CanvasGenerationMode, type CanvasNodeData, type CanvasNodeMetadata, type CanvasWorkspaceMode } from "@oc/types/canvas";
import { canvasResourceMentionToken, type CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import { useResolvedCanvasResourceReferences } from "./use-resolved-canvas-resource-references";

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
    onOpenLibrary?: () => void;
    onOpenTemplates?: () => void;
};

export function CanvasNodePromptPanel({ node, isRunning, onPromptChange, onConfigChange, onGenerate, onStop, mentionReferences = [], onImageSettingsOpenChange, onOpenLibrary, onOpenTemplates }: CanvasNodePromptPanelProps) {
    useTranslation();
    const globalConfig = useEffectiveConfig();
    const theme = canvasThemes[useCanvasColorTheme()];
    const mode = getNodeGenerationMode(node) ?? "image";
    const config = buildGenerationConfig(globalConfig, node, mode);
    const hasTextContent = node.type === CanvasNodeType.Text && Boolean(node.metadata?.content?.trim());
    const hasImageContent = node.type === CanvasNodeType.Image && Boolean(node.metadata?.content);
    const [prompt, setPrompt] = useState(node.metadata?.composerContent ?? node.metadata?.prompt ?? "");
    const [presetOpen, setPresetOpen] = useState(false);
    const [expanded, setExpanded] = useState(false);
    const [promptContentHeight, setPromptContentHeight] = useState(0);
    const resolvedMentions = useResolvedCanvasResourceReferences(mentionReferences);
    const connectedMentions = useMemo(
        () => resolvedMentions.filter((item) => item.active && item.kind !== "skill"),
        [resolvedMentions],
    );
    const videoFrameOptions = connectedMentions.filter((item) => item.kind === "image").map((item) => ({ nodeId: item.nodeId, label: item.label, title: item.title, previewUrl: item.previewUrl }));
    const hasVideoPromptTools = mode === "video" && videoFrameOptions.length > 0;
    const composerMinHeight = expanded ? 280 : 112;
    const composerHeight = Math.min(expanded ? 440 : 220, Math.max(composerMinHeight, Math.ceil(promptContentHeight + 12)));
    const visiblePrompt = stripSkillTokens(stripRecipeTokens(prompt));
    const recipeIds = listedRecipeIds(prompt);
    const skillIds = listedSkillIds(prompt);
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

    const commitPrompt = (value: string) => {
        setPrompt(value);
        onPromptChange(node.id, value);
    };

    const updateVisiblePrompt = (visible: string) => {
        commitPrompt(mergeCraftAttachments(visible, listedRecipeIds(prompt), listedSkillIds(prompt)));
        if (/(^|\s)\/[\p{L}\p{N}_-]*$/u.test(stripSkillTokens(stripRecipeTokens(visible)))) setPresetOpen(true);
    };

    const insertConnectedMention = (reference: CanvasResourceReference) => {
        const token = canvasResourceMentionToken(reference);
        if (prompt.includes(token)) return;
        const prefix = visiblePrompt.trimEnd();
        updateVisiblePrompt(`${prefix}${prefix ? " " : ""}${token} `);
    };

    const applyPreset = (preset: CanvasPromptPreset) => {
        const withoutSlash = visiblePrompt.replace(/(^|\s)\/[\p{L}\p{N}_-]*$/u, "$1").trimEnd();
        commitPrompt(mergeCraftAttachments(withoutSlash, [...recipeIds, preset.id], skillIds));
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
            {videoTools}
            <CanvasTemplateSlotBar
                theme={theme}
                metadata={node.metadata}
                model={config.model}
                onChange={(patch) => onConfigChange(node.id, patch)}
            />
            {connectedMentions.length ? (
                <div className="flex flex-wrap items-center gap-1.5 px-0.5 pb-0.5">
                    {connectedMentions.map((reference) => (
                        <ConnectedMentionChip
                            key={reference.id}
                            reference={reference}
                            inserted={prompt.includes(canvasResourceMentionToken(reference))}
                            theme={theme}
                            onInsert={() => insertConnectedMention(reference)}
                        />
                    ))}
                </div>
            ) : null}
            {recipeIds.length || skillIds.length ? (
                <div className="flex flex-wrap items-center gap-1.5 px-0.5 pb-1">
                    {recipeIds.map((id) => (
                        <CanvasCraftTokenChip
                            key={`recipe:${id}`}
                            chip={recipeAttachmentChip(id)}
                            theme={theme}
                            onRemove={() => commitPrompt(removeRecipeToken(prompt, id))}
                        />
                    ))}
                    {skillIds.map((id) => {
                        const reference = skillReferences.find((item) => item.skill?.skill_id === id);
                        return (
                            <CanvasCraftTokenChip
                                key={`skill:${id}`}
                                chip={skillAttachmentChip(id, reference?.title || reference?.label, reference?.previewUrl)}
                                theme={theme}
                                onRemove={() => commitPrompt(removeSkillToken(prompt, id))}
                            />
                        );
                    })}
                </div>
            ) : null}
            <div className="canvas-composer-field overflow-hidden" style={{ height: composerHeight, background: theme.spatial.surface }}>
                <CanvasResourceMentionTextarea
                    value={visiblePrompt}
                    references={resolvedMentions}
                    onChange={updateVisiblePrompt}
                    containerClassName="min-h-0 h-full"
                    className="thin-scrollbar h-full w-full resize-none overflow-y-auto border-none bg-transparent px-2.5 py-2 text-[var(--fs-body)] leading-5 !outline-none placeholder:text-current placeholder:opacity-35"
                    style={{ color: theme.node.text }}
                    placeholder={promptPlaceholder(mode, hasImageContent, hasTextContent, connectedMentions.length > 0)}
                    onContentSizeChange={updatePromptContentHeight}
                    onSubmit={submit}
                />
            </div>

            <div className="canvas-composer-footer">
                {onOpenTemplates ? (
                    <CanvasComposerPill
                        icon={<LayoutTemplate />}
                        label={canvasT("videoCanvas.craft.tabTemplate", "模板")}
                        title={canvasT("videoCanvas.craft.tabTemplate", "模板")}
                        aria-label={canvasT("videoCanvas.craft.tabTemplate", "模板")}
                        onClick={onOpenTemplates}
                    />
                ) : null}
                {isPortraitTexture ? (
                    <CanvasPortraitTexturePopover value={node.metadata?.portraitTexture} placement="topLeft" onChange={(portraitTexture) => onConfigChange(node.id, { portraitTexture })} />
                ) : (
                    <CanvasPresetPicker mode={mode} open={presetOpen} onOpenChange={setPresetOpen} onSelect={applyPreset} onOpenLibrary={onOpenLibrary} />
                )}
                <div className="min-w-0 flex-1">
                    <ModelPicker
                        className="!h-7 !w-full !min-w-0 !text-[var(--fs-label)]"
                        fullWidth
                        config={config}
                        value={config.model}
                        onChange={(model) => onConfigChange(node.id, { model, ...canvasModelSpecPatch(globalConfig, model, mode, {
                            size: node.metadata?.size,
                            quality: node.metadata?.quality,
                            seconds: node.metadata?.seconds,
                            vquality: node.metadata?.vquality,
                            generateAudio: node.metadata?.generateAudio,
                            watermark: node.metadata?.watermark,
                            transparentBackground: node.metadata?.transparentBackground,
                        }) })}
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
                    <>
                        <CanvasImageSettingsPopover
                            config={config}
                            placement="topLeft"
                            onConfigChange={(key, value) => onConfigChange(node.id, key === "count" ? { count: Number(value) || 1 } : { [key]: value })}
                            onOpenChange={onImageSettingsOpenChange}
                        />
                        <CanvasCameraPicker
                            rig={readCameraRig(node.metadata)}
                            placement="top"
                            onChange={(patch) => onConfigChange(node.id, patch)}
                        />
                    </>
                ) : mode === "video" ? (
                    <CanvasVideoSettingsPopover
                        config={config}
                        promptOptimize={isCanvasPromptOptimizeEnabled(node.metadata?.promptOptimize)}
                        onConfigChange={(key, value) => onConfigChange(node.id, videoConfigPatch(key, value))}
                    />
                ) : mode === "audio" ? (
                    <CanvasAudioSettingsPopover
                        config={config}
                        placement="bottomLeft"
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
                    <CanvasComposerSendButton
                        disabled={isSubmitDisabled}
                        running={isRunning}
                        onClick={() => (isRunning ? onStop(node.id) : submit())}
                    />
                </span>
            </div>
        </div>
    );
}

const MENTION_PREVIEW_SIZE = 144;
const MENTION_PREVIEW_GAP = 8;
const MENTION_PREVIEW_MARGIN = 8;
const MENTION_PREVIEW_DELAY_MS = 80;

function ConnectedMentionChip({
    reference,
    inserted,
    theme,
    onInsert,
}: {
    reference: CanvasResourceReference;
    inserted: boolean;
    theme: CanvasTheme;
    onInsert: () => void;
}) {
    const triggerRef = useRef<HTMLButtonElement>(null);
    const showTimerRef = useRef<number | null>(null);
    const [hovered, setHovered] = useState(false);
    const [position, setPosition] = useState<{ left: number; top: number } | null>(null);
    const previewUrl = reference.previewUrl;
    const canPreview = Boolean(previewUrl) && (reference.kind === "image" || reference.kind === "character" || reference.kind === "video");
    const actionLabel = inserted
        ? canvasT("videoCanvas.prompt.mentionInserted", "已引用 {{label}}", { label: reference.label })
        : canvasT("videoCanvas.prompt.mentionInsert", "引用 {{label}}", { label: reference.label });

    const clearShowTimer = () => {
        if (showTimerRef.current == null) return;
        window.clearTimeout(showTimerRef.current);
        showTimerRef.current = null;
    };

    const openPreview = () => {
        if (!canPreview) return;
        clearShowTimer();
        showTimerRef.current = window.setTimeout(() => setHovered(true), MENTION_PREVIEW_DELAY_MS);
    };

    const closePreview = () => {
        clearShowTimer();
        setHovered(false);
    };

    useEffect(() => () => {
        if (showTimerRef.current == null) return;
        window.clearTimeout(showTimerRef.current);
    }, []);

    useLayoutEffect(() => {
        if (!hovered || !canPreview) {
            setPosition(null);
            return;
        }
        const rect = triggerRef.current?.getBoundingClientRect();
        if (!rect) return;
        const size = MENTION_PREVIEW_SIZE;
        const preferAbove = rect.top - MENTION_PREVIEW_GAP - size >= MENTION_PREVIEW_MARGIN;
        const maxLeft = Math.max(MENTION_PREVIEW_MARGIN, window.innerWidth - size - MENTION_PREVIEW_MARGIN);
        const left = Math.min(Math.max(MENTION_PREVIEW_MARGIN, rect.left + rect.width / 2 - size / 2), maxLeft);
        const top = preferAbove
            ? Math.max(MENTION_PREVIEW_MARGIN, rect.top - size - MENTION_PREVIEW_GAP)
            : rect.bottom + MENTION_PREVIEW_GAP;
        setPosition({ left, top });
    }, [canPreview, hovered]);

    const preview =
        hovered && position && previewUrl && typeof document !== "undefined"
            ? createPortal(
                <div
                    data-canvas-no-zoom
                    className="canvas-overlay pointer-events-none fixed z-[var(--z-max)] overflow-hidden p-1"
                    style={{
                        ...canvasOverlayStyle(theme),
                        left: position.left,
                        top: position.top,
                        width: MENTION_PREVIEW_SIZE,
                        height: MENTION_PREVIEW_SIZE,
                    }}
                >
                    {reference.kind === "video" ? (
                        <video
                            src={previewUrl}
                            muted
                            playsInline
                            preload="metadata"
                            className="size-full rounded-[calc(var(--r-md,8px)-2px)] object-contain"
                        />
                    ) : (
                        <img
                            src={previewUrl}
                            alt={canvasT("videoCanvas.prompt.mentionPreview", "{{label}} 预览", { label: reference.label })}
                            className="size-full rounded-[calc(var(--r-md,8px)-2px)] object-contain"
                        />
                    )}
                </div>,
                getOcPortalHost(),
            )
            : null;

    return (
        <>
            <button
                ref={triggerRef}
                type="button"
                className="inline-flex h-7 max-w-full items-center gap-1.5 rounded-md border px-1.5 text-[var(--fs-label)]"
                style={{
                    background: inserted ? theme.toolbar.activeBg : theme.node.fill,
                    borderColor: theme.node.stroke,
                    color: inserted ? theme.toolbar.activeText : theme.node.text,
                }}
                aria-label={actionLabel}
                onClick={onInsert}
                onMouseEnter={openPreview}
                onMouseLeave={closePreview}
                onFocus={openPreview}
                onBlur={closePreview}
            >
                {previewUrl && (reference.kind === "image" || reference.kind === "character") ? (
                    <img src={previewUrl} alt="" className="size-5 shrink-0 rounded object-cover" />
                ) : null}
                <span className="truncate">@{reference.label}</span>
            </button>
            {preview}
        </>
    );
}

function promptPlaceholder(mode: CanvasNodeGenerationMode, hasImageContent: boolean, hasTextContent: boolean, hasConnectedMentions: boolean) {
    if (mode === "video") {
        return hasConnectedMentions
            ? canvasT("videoCanvas.prompt.placeholderVideoMention", "描述画面，输入 @ 引用已连接图片")
            : canvasT("videoCanvas.prompt.placeholderVideo", "描述画面，或选一个模板");
    }
    if (mode === "audio") return canvasT("videoCanvas.prompt.placeholderAudio", "描述要生成的音频内容");
    if (mode === "image") return hasImageContent ? canvasT("videoCanvas.prompt.placeholderImageRegen", "输入新提示词，重新生成当前图片") : canvasT("videoCanvas.prompt.placeholderImage", "描述画面，或选一个模板");
    return hasTextContent ? canvasT("videoCanvas.prompt.placeholderTextEdit", "请输入你想要将本段文本修改成什么") : canvasT("videoCanvas.prompt.placeholderText", "请输入你想要生成的文本内容");
}

function videoConfigPatch(key: CanvasVideoSettingKey, value: string) {
    if (key === "videoSeconds") return { seconds: value };
    if (key === "videoGenerateAudio") return { generateAudio: value };
    if (key === "videoWatermark") return { watermark: value };
    if (key === "count") return { count: Number(value) || 1 };
    return { [key]: value };
}

function audioConfigPatch(key: CanvasAudioSettingKey, value: string) {
    if (key === "audioVoice") return { audioVoice: value };
    if (key === "audioFormat") return { audioFormat: value };
    if (key === "audioSpeed") return { audioSpeed: value };
    return { audioInstructions: value };
}
