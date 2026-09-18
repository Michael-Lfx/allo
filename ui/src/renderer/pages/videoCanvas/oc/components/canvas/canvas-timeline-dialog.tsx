import { useEffect, useMemo, useState } from "react";
import { App } from "antd";
import { Clapperboard, Film, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { appendMediaNodeToTimeline, createEmptyTimeline } from "@oc/lib/timeline/timeline-project";
import { buildTimelineExportRequest } from "@oc/lib/timeline/timeline-export";
import { exportCanvasTimeline } from "@renderer/pages/videoCanvas/api";
import type { TimelineClip, TimelineClipKind, TimelineProject, TimelineTrackKind } from "@oc/types/timeline";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { CanvasMediaMeta } from "@renderer/pages/videoCanvas/api";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasSheet, CanvasSheetButton } from "./canvas-overlay";

type CanvasTimelineDialogProps = {
    open: boolean;
    seedNode: CanvasNodeData | null;
    nodes: CanvasNodeData[];
    timeline: TimelineProject | undefined;
    onClose: () => void;
    onSave: (timeline: TimelineProject) => void;
    onExportMedia?: (meta: CanvasMediaMeta) => void;
};

const TRACK_KINDS: TimelineTrackKind[] = ["video", "audio", "subtitle"];

/** 项目级时间线编辑壳，数据落在 doc.timeline，经 PUT /doc 持久化。 */
export function CanvasTimelineDialog({ open, seedNode, nodes, timeline, onClose, onSave, onExportMedia }: CanvasTimelineDialogProps) {
    useTranslation();
    const { message } = App.useApp();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [draft, setDraft] = useState<TimelineProject>(() => timeline || createEmptyTimeline());
    const [exporting, setExporting] = useState(false);

    useEffect(() => {
        if (!open) return;
        let next = timeline || createEmptyTimeline();
        if (seedNode && (seedNode.type === CanvasNodeType.Video || seedNode.type === CanvasNodeType.Audio) && !next.clips.some((clip) => clip.nodeId === seedNode.id)) {
            next = appendMediaNodeToTimeline(next, seedNode);
        }
        setDraft(next);
    }, [open, seedNode, timeline]);

    const mediaNodes = useMemo(
        () => nodes.filter((node) => (node.type === CanvasNodeType.Video || node.type === CanvasNodeType.Audio) && Boolean(node.metadata?.content)),
        [nodes],
    );

    const save = () => {
        onSave({ ...draft, updatedAt: new Date().toISOString() });
        message.success(canvasT("videoCanvas.timeline.saved", "已保存时间线（{{count}} 个片段）", { count: draft.clips.length }));
        onClose();
    };

    const exportFilm = async () => {
        const request = buildTimelineExportRequest(draft, nodes, seedNode?.title ? `${seedNode.title} 成片` : undefined);
        if ("error" in request) {
            message.warning(request.error === "no-clips"
                ? canvasT("videoCanvas.timeline.exportNeedClips", "请先添加至少一个视频片段")
                : canvasT("videoCanvas.timeline.exportNeedMedia", "片段缺少本地媒体，无法导出"));
            return;
        }
        setExporting(true);
        try {
            const meta = await exportCanvasTimeline(request);
            onExportMedia?.(meta);
            message.success(canvasT("videoCanvas.timeline.exported", "已导出成片并添加到画布"));
            onClose();
        } catch (error) {
            message.error(formatCanvasUserError(error, canvasT("videoCanvas.timeline.exportFailed", "时间线导出失败")));
        } finally {
            setExporting(false);
        }
    };

    const duration = Math.max(draft.durationMs, 1);
    const clipColor = (kind: TimelineTrackKind) => (kind === "video" ? theme.accent.primary : kind === "audio" ? theme.node.activeStroke : theme.node.muted);

    return (
        <CanvasSheet
            open={open}
            theme={theme}
            width="min(820px, 94vw)"
            title={canvasT("videoCanvas.timeline.title", "时间线")}
            subtitle={canvasT("videoCanvas.timeline.summary", "{{tracks}} 条轨道 · {{clips}} 个片段 · {{seconds}}s", { tracks: draft.tracks.length, clips: draft.clips.length, seconds: (draft.durationMs / 1000).toFixed(1) })}
            onClose={onClose}
            footer={
                <>
                    <CanvasSheetButton theme={theme} onClick={onClose}>{canvasT("videoCanvas.timeline.cancel", "取消")}</CanvasSheetButton>
                    <span className="flex-1" />
                    <CanvasSheetButton theme={theme} disabled={exporting} onClick={() => void exportFilm()}>
                        <Film className="size-3.5" />
                        {exporting ? canvasT("videoCanvas.timeline.exporting", "正在导出…") : canvasT("videoCanvas.timeline.export", "导出成片")}
                    </CanvasSheetButton>
                    <CanvasSheetButton theme={theme} variant="primary" onClick={save}>{canvasT("videoCanvas.timeline.save", "保存到画布文档")}</CanvasSheetButton>
                </>
            }
        >
            <div className="mb-3 flex flex-wrap items-center gap-2 text-sm" style={{ color: theme.node.muted }}>
                <Clapperboard className="size-4" />
                <span>{canvasT("videoCanvas.timeline.addHint", "从画布添加视频 / 音频节点")}</span>
            </div>
            <div className="mb-3 flex flex-wrap gap-2">
                {mediaNodes.map((node) => (
                    <CanvasSheetButton
                        key={node.id}
                        theme={theme}
                        disabled={draft.clips.some((clip) => clip.nodeId === node.id)}
                        onClick={() => setDraft((current) => appendMediaNodeToTimeline(current, node))}
                    >
                        <Plus className="size-3.5" />
                        {node.title || (node.type === CanvasNodeType.Audio ? canvasT("videoCanvas.timeline.audio", "音频") : canvasT("videoCanvas.timeline.video", "视频"))}
                    </CanvasSheetButton>
                ))}
            </div>
            <div className="space-y-2 rounded-[var(--r-lg)] border px-3 py-2.5" style={{ borderColor: theme.toolbar.border, background: theme.node.fill }}>
                {TRACK_KINDS.map((kind) => {
                    const track = draft.tracks.find((item) => item.kind === kind);
                    const clips = track ? draft.clips.filter((clip) => clip.trackId === track.id) : [];
                    return (
                        <div key={kind} className="grid grid-cols-[52px_minmax(0,1fr)] items-center gap-2">
                            <span className="text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.muted }}>{trackKindLabel(kind)}</span>
                            <div className="relative h-7 overflow-hidden rounded-md" style={{ background: theme.toolbar.itemHover }}>
                                {clips.map((clip) => (
                                    <div
                                        key={clip.id}
                                        className="absolute inset-y-1 rounded-sm"
                                        style={{
                                            left: `${(clip.startMs / duration) * 100}%`,
                                            width: `${Math.max(1.5, (clip.durationMs / duration) * 100)}%`,
                                            background: clipColor(kind),
                                            opacity: 0.72,
                                        }}
                                        title={clip.title || clip.nodeId}
                                    />
                                ))}
                            </div>
                        </div>
                    );
                })}
            </div>
            <div className="thin-scrollbar mt-3 max-h-[32vh] space-y-2 overflow-y-auto">
                {draft.clips.length ? draft.clips.map((clip) => (
                    <ClipRow key={clip.id} clip={clip} theme={theme} onRemove={() => setDraft((current) => {
                        const clips = current.clips.filter((item) => item.id !== clip.id);
                        return { ...current, clips, durationMs: clips.reduce((max, item) => Math.max(max, item.startMs + item.durationMs), 0) };
                    })} />
                )) : <div className="rounded-md border border-dashed px-3 py-8 text-center text-sm" style={{ borderColor: theme.toolbar.border, color: theme.node.muted }}>{canvasT("videoCanvas.timeline.empty", "从上方按钮添加画布中的视频/音频节点")}</div>}
            </div>
        </CanvasSheet>
    );
}

function ClipRow({ clip, theme, onRemove }: { clip: TimelineClip; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onRemove: () => void }) {
    return (
        <div className="flex items-center justify-between gap-2 rounded-md border px-3 py-2 text-sm" style={{ borderColor: theme.toolbar.border }}>
            <div className="min-w-0">
                <div className="truncate font-medium">{clip.title || clip.nodeId}</div>
                <div style={{ color: theme.node.muted }}>{trackKindLabel(clip.kind)} · {(clip.startMs / 1000).toFixed(1)}s → {((clip.startMs + clip.durationMs) / 1000).toFixed(1)}s</div>
            </div>
            <CanvasSheetButton theme={theme} variant="danger" onClick={onRemove}>{canvasT("videoCanvas.timeline.remove", "移除")}</CanvasSheetButton>
        </div>
    );
}

function trackKindLabel(kind: TimelineTrackKind | TimelineClipKind) {
    if (kind === "audio") return canvasT("videoCanvas.timeline.trackAudio", "音频");
    if (kind === "subtitle") return canvasT("videoCanvas.timeline.trackSubtitle", "字幕");
    return canvasT("videoCanvas.timeline.trackVideo", "视频");
}
