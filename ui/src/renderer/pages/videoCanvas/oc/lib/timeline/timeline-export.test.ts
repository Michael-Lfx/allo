import { describe, expect, test } from "bun:test";

import { buildTimelineExportRequest } from "@oc/lib/timeline/timeline-export";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import type { TimelineClip, TimelineProject } from "@oc/types/timeline";

function videoNode(id: string, mediaId: string): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Video,
        title: id,
        position: { x: 0, y: 0 },
        width: 480,
        height: 270,
        metadata: { mediaId, content: `/api/video-canvas/media/${mediaId}` },
    };
}

function clip(id: string, nodeId: string, startMs: number, durationMs: number, sourceStartMs = 0): TimelineClip {
    return {
        id,
        kind: "video",
        nodeId,
        trackId: "video",
        startMs,
        durationMs,
        title: id,
        sourceStartMs,
        sourceDurationMs: durationMs,
    };
}

function timeline(clips: TimelineClip[]): TimelineProject {
    return {
        version: 2,
        tracks: [],
        clips,
        durationMs: clips.reduce((max, item) => Math.max(max, item.startMs + item.durationMs), 0),
    };
}

describe("buildTimelineExportRequest", () => {
    test("emits structured clips and gap_before_ms for holes over 100ms", () => {
        const nodes = [videoNode("a", "media-a"), videoNode("b", "media-b")];
        const request = buildTimelineExportRequest(
            timeline([clip("c1", "a", 0, 1000), clip("c2", "b", 1500, 800, 200)]),
            nodes,
            "成片",
        );
        expect("error" in request).toBe(false);
        if ("error" in request) return;
        expect(request.clips).toEqual([
            { media_id: "media-a", source_start_ms: 0, duration_ms: 1000 },
            { media_id: "media-b", source_start_ms: 200, duration_ms: 800, gap_before_ms: 500 },
        ]);
        expect(request.title).toBe("成片");
    });

    test("returns missing-media when a clip has no local media id", () => {
        const nodes: CanvasNodeData[] = [
            { id: "a", type: CanvasNodeType.Video, title: "a", position: { x: 0, y: 0 }, width: 100, height: 100 },
        ];
        expect(buildTimelineExportRequest(timeline([clip("c1", "a", 0, 1000)]), nodes)).toEqual({ error: "missing-media" });
    });
});
