import { resourceIdFromStorageKey } from "@oc/services/api/resources";
import { canvasMediaUrl, extractMediaIdFromCanvasMediaUrl, resolveCanvasUrl } from "@renderer/pages/videoCanvas/api";
import type { CanvasNodeData } from "@oc/types/canvas";

/** Resolve the local `/api/video-canvas/media/{id}` id from a canvas node. */
export function canvasNodeMediaId(node: CanvasNodeData | undefined | null): string | null {
    if (!node) return null;
    const explicit = node.metadata?.mediaId?.trim();
    if (explicit) return explicit;
    const fromKey = resourceIdFromStorageKey(node.metadata?.storageKey);
    if (fromKey) return fromKey;
    const url = (node.metadata?.content || "").trim();
    if (url.startsWith("resource:")) {
        const fromContent = resourceIdFromStorageKey(url);
        if (fromContent) return fromContent;
    }
    const match = url.match(/\/api\/video-canvas\/media\/([^/?#]+)/);
    return match?.[1] ? decodeURIComponent(match[1]) : null;
}

/**
 * Turn a persisted src into something the current session can load.
 * Historical docs often store `blob:`, `resource:id`, or
 * `http://127.0.0.1:{oldPort}/api/video-canvas/media/{id}` from a previous
 * desktop launch — those 404 after restart. Rewrite media ids against
 * `getBaseUrl()`; drop one-shot blob URLs.
 */
export function rewriteCanvasDisplayUrl(path: string | null | undefined): string {
    const trimmed = path?.trim() || "";
    if (!trimmed || trimmed.startsWith("blob:")) return "";
    if (/^data:/i.test(trimmed)) return trimmed;
    if (trimmed.startsWith("resource:")) {
        const id = resourceIdFromStorageKey(trimmed);
        return id ? canvasMediaUrl(id) : "";
    }
    const mediaId = extractMediaIdFromCanvasMediaUrl(trimmed);
    if (mediaId) return canvasMediaUrl(mediaId);
    if (/^https?:\/\//i.test(trimmed)) return trimmed;
    return resolveCanvasUrl(trimmed) || trimmed;
}

/** HTTP/data src for painting a node. Never returns a dead blob: or old-port media URL. */
export function canvasNodeDisplayUrl(node: CanvasNodeData | undefined | null): string {
    if (!node) return "";
    const mediaId = canvasNodeMediaId(node);
    if (mediaId) return canvasMediaUrl(mediaId);
    return rewriteCanvasDisplayUrl(node.metadata?.content);
}
