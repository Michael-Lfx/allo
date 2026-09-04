import { canvasNodeMediaId } from "@oc/lib/canvas/canvas-media-id";
import { serializeSrtEntries } from "@oc/lib/timeline/srt-parser";
import { buildSubtitleSrt, getOrderedVideoClips } from "@oc/lib/timeline/timeline-to-ffmpeg";
import type { CanvasNodeData } from "@oc/types/canvas";
import type { SrtEntry, TimelineProject } from "@oc/types/timeline";

export type TimelineExportClip = {
    media_id: string;
    source_start_ms?: number;
    duration_ms: number;
    gap_before_ms?: number;
};

export type TimelineExportRequest = {
    clips: TimelineExportClip[];
    srt?: string;
    burn_subtitles?: boolean;
    title?: string;
};

export function buildTimelineExportRequest(
    timeline: TimelineProject,
    nodes: CanvasNodeData[],
    title?: string,
): TimelineExportRequest | { error: string } {
    const nodeById = new Map(nodes.map((node) => [node.id, node]));
    const clips: TimelineExportClip[] = [];
    let cursorMs = 0;
    for (const clip of getOrderedVideoClips(timeline)) {
        const node = nodeById.get(clip.nodeId);
        const mediaId = canvasNodeMediaId(node);
        if (!mediaId) {
            return { error: "missing-media" };
        }
        const gapMs = Math.max(0, clip.startMs - cursorMs);
        clips.push({
            media_id: mediaId,
            source_start_ms: clip.sourceStartMs || 0,
            duration_ms: clip.durationMs,
            ...(gapMs > 100 ? { gap_before_ms: gapMs } : {}),
        });
        cursorMs = clip.startMs + clip.durationMs;
    }
    if (!clips.length) return { error: "no-clips" };
    const srt = srtForTimelineExport(timeline, nodes);
    return {
        clips,
        ...(srt ? { srt, burn_subtitles: true } : {}),
        title: title || "timeline-export",
    };
}

function srtForTimelineExport(timeline: TimelineProject, nodes: CanvasNodeData[]): string {
    const fromTimeline = buildSubtitleSrt(timeline.clips).trim();
    if (fromTimeline) return fromTimeline;
    const nodeById = new Map(nodes.map((node) => [node.id, node]));
    const entries: SrtEntry[] = [];
    for (const clip of getOrderedVideoClips(timeline)) {
        const node = nodeById.get(clip.nodeId);
        const sourceStart = clip.sourceStartMs || 0;
        for (const entry of node?.metadata?.subtitleEntries || []) {
            const start = clip.startMs + Math.max(0, entry.startMs - sourceStart);
            const end = clip.startMs + Math.max(0, entry.endMs - sourceStart);
            if (end <= clip.startMs || start >= clip.startMs + clip.durationMs) continue;
            entries.push({
                index: entries.length + 1,
                startMs: Math.max(start, clip.startMs),
                endMs: Math.min(end, clip.startMs + clip.durationMs),
                text: entry.text,
            });
        }
    }
    return serializeSrtEntries(entries).trim();
}
