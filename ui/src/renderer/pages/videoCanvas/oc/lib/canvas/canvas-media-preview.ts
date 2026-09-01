import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

/**
 * Returns an image source that is safe to mount for a passive video preview.
 * The original video URL is deliberately never returned: callers must fall
 * back to a video icon instead of creating a decoder outside the active node.
 *
 * LibTV/OSS snapshot URLs are not used in allo; posters live in canvas JSON
 * (`videoPreview` / `previewContent`) after client hydration.
 */
export function canvasNodeVideoPreviewUrl(node: CanvasNodeData) {
    if (node.type !== CanvasNodeType.Video) return "";
    const content = node.metadata?.content || "";
    const generatedPreview = node.metadata?.videoPreview?.content || "";
    if (generatedPreview && generatedPreview !== content) return generatedPreview;
    const explicitPreview = node.metadata?.previewContent || "";
    if (explicitPreview && explicitPreview !== content) return explicitPreview;
    return "";
}

export function canvasVideoAssetPreviewUrl(videoUrl: string, coverUrl?: string) {
    const explicitCover = coverUrl?.trim() || "";
    if (explicitCover && explicitCover !== videoUrl) return explicitCover;
    return "";
}
