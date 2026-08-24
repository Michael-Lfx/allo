import { nanoid } from "nanoid";

import type { TimelineClip, TimelineProject, TimelineTrack } from "@oc/types/timeline";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

export function createEmptyTimeline(): TimelineProject {
    const videoTrack: TimelineTrack = { id: `track-${nanoid(8)}`, kind: "video", label: "视频", order: 0 };
    const audioTrack: TimelineTrack = { id: `track-${nanoid(8)}`, kind: "audio", label: "音频", order: 1 };
    const subtitleTrack: TimelineTrack = { id: `track-${nanoid(8)}`, kind: "subtitle", label: "字幕", order: 2 };
    return {
        version: 2,
        tracks: [videoTrack, audioTrack, subtitleTrack],
        clips: [],
        durationMs: 0,
        updatedAt: new Date().toISOString(),
    };
}

/** 将视频/音频节点追加到时间线（client-doc MVP，不做复杂排布）。 */
export function appendMediaNodeToTimeline(timeline: TimelineProject, node: CanvasNodeData): TimelineProject {
    const kind = node.type === CanvasNodeType.Audio ? "audio" : node.type === CanvasNodeType.Video ? "video" : null;
    if (!kind) return timeline;
    const track = timeline.tracks.find((item) => item.kind === kind) || timeline.tracks[0];
    if (!track) return timeline;
    const durationMs = Math.max(1000, Math.floor(Number(node.metadata?.durationMs) || 5000));
    const startMs = timeline.clips.filter((clip) => clip.trackId === track.id).reduce((max, clip) => Math.max(max, clip.startMs + clip.durationMs), 0);
    const clip: TimelineClip = {
        id: `clip-${nanoid(8)}`,
        kind,
        nodeId: node.id,
        trackId: track.id,
        startMs,
        durationMs,
        title: node.title,
        sourceDurationMs: durationMs,
    };
    const clips = [...timeline.clips, clip];
    const nextDuration = clips.reduce((max, item) => Math.max(max, item.startMs + item.durationMs), 0);
    return {
        ...timeline,
        clips,
        durationMs: nextDuration,
        updatedAt: new Date().toISOString(),
    };
}
