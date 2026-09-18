import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

import { rewriteCanvasDisplayUrl } from "./canvas-media-id";

/**
 * Returns an image source that is safe to mount for a passive video preview.
 * The original video URL is deliberately never returned: callers must fall
 * back to a video icon (or a muted first-frame element) instead of treating
 * the video file as an <img> src.
 *
 * LibTV/OSS snapshot URLs are not used in allo; posters live in canvas JSON
 * (`videoPreview` / `previewContent`) after client hydration.
 */
export function canvasNodeVideoPreviewUrl(node: CanvasNodeData) {
    if (node.type !== CanvasNodeType.Video) return "";
    const content = node.metadata?.content || "";
    const contentDisplay = rewriteCanvasDisplayUrl(content);
    const generatedPreview = node.metadata?.videoPreview?.content || "";
    const generated = usablePosterUrl(generatedPreview, content, contentDisplay);
    if (generated) return generated;
    const explicitPreview = node.metadata?.previewContent || "";
    return usablePosterUrl(explicitPreview, content, contentDisplay);
}

export function canvasVideoAssetPreviewUrl(videoUrl: string, coverUrl?: string) {
    const explicitCover = coverUrl?.trim() || "";
    if (explicitCover && explicitCover !== videoUrl) return rewriteCanvasDisplayUrl(explicitCover);
    return "";
}

function usablePosterUrl(candidate: string, content: string, contentDisplay: string) {
    if (!candidate || candidate === content) return "";
    const rewritten = rewriteCanvasDisplayUrl(candidate);
    if (!rewritten || rewritten === contentDisplay) return "";
    return rewritten;
}
