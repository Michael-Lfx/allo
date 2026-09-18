import { useEffect, useRef, useState } from "react";
import { App } from "antd";
import { ChevronDown, FileDown, FileUp, Plus, Trash2, WandSparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { VideoPlayer } from "@oc/components/video-player";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { canvasNodeDisplayUrl, canvasNodeMediaId } from "@oc/lib/canvas/canvas-media-id";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { formatSubtitleClock, parseSrt, parseSubtitleClock, serializeSrtEntries } from "@oc/lib/timeline/srt-parser";
import { transcriptToSrtEntries } from "@oc/lib/timeline/transcript-to-srt";
import { transcribeCanvasMedia } from "@renderer/pages/videoCanvas/api";
import { createDefaultSubtitleStyle, type SrtEntry, type SubtitlePosition, type SubtitleStyle } from "@oc/types/timeline";
import type { CanvasNodeData, CanvasNodeMetadata } from "@oc/types/canvas";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { ChoiceChip } from "@oc/components/generation-settings-chrome";
import { CanvasRange, CanvasSheet, CanvasSheetButton } from "./canvas-overlay";

type CanvasSubtitleDialogProps = {
    node: CanvasNodeData;
    open: boolean;
    onClose: () => void;
    onSave: (nodeId: string, patch: Partial<CanvasNodeMetadata>) => void;
};

/** SRT 导入/导出与条目编辑，写回节点 metadata（经现有 PUT /doc 持久化）。 */
export function CanvasSubtitleDialog({ node, open, onClose, onSave }: CanvasSubtitleDialogProps) {
    useTranslation();
    const { message } = App.useApp();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [entries, setEntries] = useState<SrtEntry[]>([]);
    const [style, setStyle] = useState<SubtitleStyle>(createDefaultSubtitleStyle());
    const [styleOpen, setStyleOpen] = useState(false);
    const [transcribing, setTranscribing] = useState(false);
    const fileInputRef = useRef<HTMLInputElement>(null);
    const mediaId = canvasNodeMediaId(node);
    const previewUrl = canvasNodeDisplayUrl(node);

    useEffect(() => {
        if (!open) return;
        setEntries(node.metadata?.subtitleEntries || []);
        setStyle(node.metadata?.subtitleStyle || createDefaultSubtitleStyle());
        setStyleOpen(false);
    }, [open, node]);

    const save = () => {
        onSave(node.id, {
            subtitleEntries: entries,
            subtitleStyle: style,
            subtitleUpdatedAt: new Date().toISOString(),
        });
        message.success(entries.length
            ? canvasT("videoCanvas.subtitle.saved", "已保存 {{count}} 条字幕", { count: entries.length })
            : canvasT("videoCanvas.subtitle.cleared", "已清空字幕"));
        onClose();
    };

    const importSrt = async (file: File) => {
        try {
            const text = await file.text();
            const parsed = parseSrt(text);
            if (!parsed.length) {
                message.warning(canvasT("videoCanvas.subtitle.parseEmpty", "未解析到有效字幕条目"));
                return;
            }
            setEntries(parsed);
            message.success(canvasT("videoCanvas.subtitle.saved", "已保存 {{count}} 条字幕", { count: parsed.length }));
        } catch (error) {
            message.error(formatCanvasUserError(error, canvasT("videoCanvas.subtitle.importFailed", "导入失败")));
        }
    };

    const exportSrt = () => {
        const blob = new Blob([serializeSrtEntries(entries)], { type: "text/plain;charset=utf-8" });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = `${node.title || "subtitles"}.srt`;
        anchor.click();
        URL.revokeObjectURL(url);
    };

    const transcribe = async () => {
        if (!mediaId) {
            message.warning(canvasT("videoCanvas.subtitle.transcribeNeedMedia", "当前节点没有可转写的本地媒体"));
            return;
        }
        setTranscribing(true);
        try {
            const result = await transcribeCanvasMedia(mediaId);
            const next = transcriptToSrtEntries(result.text, result.duration_ms ?? node.metadata?.durationMs);
            if (!next.length) {
                message.warning(canvasT("videoCanvas.subtitle.parseEmpty", "未解析到有效字幕条目"));
                return;
            }
            setEntries(next);
            message.success(canvasT("videoCanvas.subtitle.transcribed", "已转写 {{count}} 条字幕", { count: next.length }));
        } catch (error) {
            message.error(formatCanvasUserError(error, canvasT("videoCanvas.subtitle.transcribeFailed", "转写失败")));
        } finally {
            setTranscribing(false);
        }
    };

    const positions: SubtitlePosition[] = ["top", "center", "bottom"];

    return (
        <CanvasSheet
            open={open}
            theme={theme}
            width="min(960px, 94vw)"
            title={canvasT("videoCanvas.subtitle.title", "字幕 · {{name}}", { name: node.title || canvasT("videoCanvas.subtitle.videoFallback", "视频") })}
            onClose={onClose}
            footer={
                <>
                    <CanvasSheetButton theme={theme} onClick={() => fileInputRef.current?.click()}>
                        <FileUp className="size-3.5" />
                        {canvasT("videoCanvas.subtitle.import", "导入 SRT")}
                    </CanvasSheetButton>
                    <CanvasSheetButton theme={theme} disabled={!entries.length} onClick={exportSrt}>
                        <FileDown className="size-3.5" />
                        {canvasT("videoCanvas.subtitle.export", "导出 SRT")}
                    </CanvasSheetButton>
                    <span title={!mediaId ? canvasT("videoCanvas.subtitle.transcribeNeedMedia", "当前节点没有可转写的本地媒体") : undefined}>
                        <CanvasSheetButton theme={theme} disabled={!mediaId || transcribing} onClick={() => void transcribe()}>
                            <WandSparkles className="size-3.5" />
                            {transcribing ? canvasT("videoCanvas.subtitle.transcribing", "正在转写…") : canvasT("videoCanvas.subtitle.transcribe", "转写自动字幕")}
                        </CanvasSheetButton>
                    </span>
                    <span className="flex-1" />
                    <CanvasSheetButton theme={theme} onClick={onClose}>{canvasT("videoCanvas.subtitle.cancel", "取消")}</CanvasSheetButton>
                    <CanvasSheetButton theme={theme} variant="primary" onClick={save}>{canvasT("videoCanvas.subtitle.save", "保存到节点")}</CanvasSheetButton>
                </>
            }
        >
            <div className="grid gap-3 lg:grid-cols-[minmax(280px,1fr)_minmax(320px,1.1fr)]">
                <div className="overflow-hidden rounded-[var(--r-lg)] bg-black">
                    {previewUrl ? (
                        <VideoPlayer
                            src={previewUrl}
                            mimeType={node.metadata?.mimeType}
                            title={node.title || canvasT("videoCanvas.subtitle.videoFallback", "视频")}
                            className="max-h-[42vh] w-full"
                            subtitleEntries={entries}
                            subtitleStyle={style}
                            subtitleLabel={canvasT("videoCanvas.subtitle.videoFallback", "视频")}
                        />
                    ) : (
                        <div className="grid h-44 place-items-center text-xs" style={{ color: theme.node.muted }}>
                            {canvasT("videoCanvas.subtitle.noPreview", "当前节点没有可预览的视频")}
                        </div>
                    )}
                </div>
                <div className="flex min-h-0 flex-col gap-2">
                    <button
                        type="button"
                        className="flex h-7 items-center gap-1 text-left text-[var(--fs-tiny)] font-medium"
                        style={{ color: theme.node.muted }}
                        onClick={() => setStyleOpen((value) => !value)}
                    >
                        <ChevronDown className={`size-3.5 transition-transform ${styleOpen ? "rotate-0" : "-rotate-90"}`} />
                        {canvasT("videoCanvas.subtitle.style", "字幕样式")}
                    </button>
                    {styleOpen ? (
                        <div className="grid gap-2 rounded-[var(--r-md)] border px-2.5 py-2" style={{ borderColor: theme.toolbar.border }}>
                            <div className="flex flex-wrap items-center gap-1.5">
                                <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.subtitle.position", "位置")}</span>
                                {positions.map((position) => (
                                    <ChoiceChip key={position} selected={style.position === position} theme={theme} onClick={() => setStyle((current) => ({ ...current, position }))}>
                                        {position === "top"
                                            ? canvasT("videoCanvas.subtitle.positionTop", "顶部")
                                            : position === "center"
                                              ? canvasT("videoCanvas.subtitle.positionCenter", "居中")
                                              : canvasT("videoCanvas.subtitle.positionBottom", "底部")}
                                    </ChoiceChip>
                                ))}
                            </div>
                            <div className="grid grid-cols-[auto_minmax(0,1fr)_48px] items-center gap-2">
                                <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.subtitle.fontSize", "字号")}</span>
                                <CanvasRange theme={theme} min={12} max={48} value={style.fontSize} ariaLabel={canvasT("videoCanvas.subtitle.fontSize", "字号")} onChange={(fontSize) => setStyle((current) => ({ ...current, fontSize }))} />
                                <span className="text-right text-[var(--fs-tiny)] tabular-nums">{style.fontSize}</span>
                            </div>
                            <label className="flex items-center gap-2 text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>
                                {canvasT("videoCanvas.subtitle.color", "颜色")}
                                <input
                                    type="color"
                                    className="h-7 w-10 cursor-pointer rounded border-0 bg-transparent"
                                    value={style.color}
                                    onChange={(event) => setStyle((current) => ({ ...current, color: event.target.value }))}
                                />
                            </label>
                        </div>
                    ) : null}
                    <div className="flex items-center justify-between gap-2">
                        <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>
                            {canvasT("videoCanvas.subtitle.saved", "已保存 {{count}} 条字幕", { count: entries.length })}
                        </span>
                        <CanvasSheetButton
                            theme={theme}
                            onClick={() => setEntries((current) => [...current, { index: current.length + 1, startMs: current.at(-1)?.endMs || 0, endMs: (current.at(-1)?.endMs || 0) + 2000, text: "" }])}
                        >
                            <Plus className="size-3.5" />
                            {canvasT("videoCanvas.subtitle.add", "添加条目")}
                        </CanvasSheetButton>
                    </div>
                    <div className="thin-scrollbar max-h-[38vh] space-y-2 overflow-y-auto">
                        {entries.length ? entries.map((entry, index) => (
                            <div key={`${entry.index}-${index}`} className="grid grid-cols-[76px_76px_minmax(0,1fr)_28px] items-center gap-1.5">
                                <input
                                    className="canvas-sheet-input"
                                    value={formatSubtitleClock(entry.startMs)}
                                    aria-label={canvasT("videoCanvas.subtitle.startAria", "开始时间")}
                                    onChange={(event) => setEntries((current) => current.map((item, i) => (i === index ? { ...item, startMs: parseSubtitleClock(event.target.value, item.startMs) } : item)))}
                                />
                                <input
                                    className="canvas-sheet-input"
                                    value={formatSubtitleClock(entry.endMs)}
                                    aria-label={canvasT("videoCanvas.subtitle.endAria", "结束时间")}
                                    onChange={(event) => setEntries((current) => current.map((item, i) => (i === index ? { ...item, endMs: parseSubtitleClock(event.target.value, item.endMs) } : item)))}
                                />
                                <input
                                    className="canvas-sheet-input"
                                    value={entry.text}
                                    onChange={(event) => setEntries((current) => current.map((item, i) => (i === index ? { ...item, text: event.target.value } : item)))}
                                    placeholder={canvasT("videoCanvas.subtitle.placeholder", "字幕文本")}
                                />
                                <button
                                    type="button"
                                    className="canvas-chrome-token is-icon"
                                    aria-label={canvasT("videoCanvas.subtitle.remove", "删除条目")}
                                    onClick={() => setEntries((current) => current.filter((_, i) => i !== index).map((item, i) => ({ ...item, index: i + 1 })))}
                                >
                                    <Trash2 className="size-3.5" />
                                </button>
                            </div>
                        )) : <div className="rounded-md border border-dashed px-3 py-8 text-center text-sm" style={{ borderColor: theme.toolbar.border, color: theme.node.muted }}>{canvasT("videoCanvas.subtitle.empty", "尚无字幕，可导入 SRT 或手动添加")}</div>}
                    </div>
                </div>
            </div>
            <input ref={fileInputRef} type="file" accept=".srt,text/plain" className="hidden" onChange={(event) => { const file = event.target.files?.[0]; if (file) void importSrt(file); event.target.value = ""; }} />
        </CanvasSheet>
    );
}
