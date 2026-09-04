import { useEffect, useMemo, useState } from "react";
import { App, Button, Modal } from "antd";
import { Clapperboard, Film, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { appendMediaNodeToTimeline, createEmptyTimeline } from "@oc/lib/timeline/timeline-project";
import { buildTimelineExportRequest } from "@oc/lib/timeline/timeline-export";
import { exportCanvasTimeline } from "@renderer/pages/videoCanvas/api";
import type { TimelineProject } from "@oc/types/timeline";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { CanvasMediaMeta } from "@renderer/pages/videoCanvas/api";

type CanvasTimelineDialogProps = {
    open: boolean;
    seedNode: CanvasNodeData | null;
    nodes: CanvasNodeData[];
    timeline: TimelineProject | undefined;
    onClose: () => void;
    onSave: (timeline: TimelineProject) => void;
    onExportMedia?: (meta: CanvasMediaMeta) => void;
};

/** MVP：项目级时间线编辑壳，数据落在 doc.timeline，经 PUT /doc 持久化。 */
export function CanvasTimelineDialog({ open, seedNode, nodes, timeline, onClose, onSave, onExportMedia }: CanvasTimelineDialogProps) {
    useTranslation();
    const { message } = App.useApp();
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

    return (
        <Modal
            title={canvasT("videoCanvas.timeline.title", "时间线")}
            open={open}
            onCancel={onClose}
            width="min(760px, 94vw)"
            centered
            footer={[
                <Button key="cancel" onClick={onClose}>{canvasT("videoCanvas.timeline.cancel", "取消")}</Button>,
                <Button key="export" icon={<Film className="size-3.5" />} loading={exporting} onClick={() => void exportFilm()}>{canvasT("videoCanvas.timeline.export", "导出成片")}</Button>,
                <Button key="save" type="primary" onClick={save}>{canvasT("videoCanvas.timeline.save", "保存到画布文档")}</Button>,
            ]}
        >
            <div className="mb-3 flex flex-wrap items-center gap-2 text-sm opacity-70">
                <Clapperboard className="size-4" />
                <span>{canvasT("videoCanvas.timeline.summary", "{{tracks}} 条轨道 · {{clips}} 个片段 · {{seconds}}s", { tracks: draft.tracks.length, clips: draft.clips.length, seconds: (draft.durationMs / 1000).toFixed(1) })}</span>
            </div>
            <div className="mb-3 flex flex-wrap gap-2">
                {mediaNodes.map((node) => (
                    <Button
                        key={node.id}
                        size="small"
                        icon={<Plus className="size-3.5" />}
                        disabled={draft.clips.some((clip) => clip.nodeId === node.id)}
                        onClick={() => setDraft((current) => appendMediaNodeToTimeline(current, node))}
                    >
                        {node.title || (node.type === CanvasNodeType.Audio ? canvasT("videoCanvas.timeline.audio", "音频") : canvasT("videoCanvas.timeline.video", "视频"))}
                    </Button>
                ))}
            </div>
            <div className="thin-scrollbar max-h-[46vh] space-y-2 overflow-y-auto">
                {draft.clips.length ? draft.clips.map((clip) => {
                    const track = draft.tracks.find((item) => item.id === clip.trackId);
                    return (
                        <div key={clip.id} className="flex items-center justify-between rounded-md border px-3 py-2 text-sm">
                            <div className="min-w-0">
                                <div className="truncate font-medium">{clip.title || clip.nodeId}</div>
                                <div className="opacity-55">{track?.label || clip.kind} · {(clip.startMs / 1000).toFixed(1)}s → {((clip.startMs + clip.durationMs) / 1000).toFixed(1)}s</div>
                            </div>
                            <Button type="link" danger onClick={() => setDraft((current) => {
                                const clips = current.clips.filter((item) => item.id !== clip.id);
                                return { ...current, clips, durationMs: clips.reduce((max, item) => Math.max(max, item.startMs + item.durationMs), 0) };
                            })}>{canvasT("videoCanvas.timeline.remove", "移除")}</Button>
                        </div>
                    );
                }) : <div className="rounded-md border border-dashed px-3 py-8 text-center text-sm opacity-60">{canvasT("videoCanvas.timeline.empty", "从上方按钮添加画布中的视频/音频节点")}</div>}
            </div>
        </Modal>
    );
}
