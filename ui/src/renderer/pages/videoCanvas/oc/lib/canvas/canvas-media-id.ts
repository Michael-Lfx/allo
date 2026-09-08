import { resourceIdFromStorageKey, resourceStorageKey } from "@oc/services/api/resources";
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

/**
 * Paint src for a reference still. Media nodes keep bytes on `storageKey`/`url`
 * (`dataUrl` is often empty); this is the display-only counterpart.
 */
export function referenceImagePreviewUrl(image?: { dataUrl?: string; url?: string; storageKey?: string } | null): string {
    if (!image) return "";
    const dataUrl = image.dataUrl?.trim() || "";
    if (/^data:/i.test(dataUrl)) return dataUrl;
    const mediaId = resourceIdFromStorageKey(image.storageKey) || extractMediaIdFromCanvasMediaUrl(image.url) || extractMediaIdFromCanvasMediaUrl(dataUrl);
    if (mediaId) return canvasMediaUrl(mediaId);
    return rewriteCanvasDisplayUrl(image.url) || rewriteCanvasDisplayUrl(dataUrl) || usableCanvasSessionUrl(dataUrl) || usableCanvasSessionUrl(image.url);
}

/**
 * Fields for model reference uploads. Inline `data:` stays in `dataUrl`;
 * canvas media goes through `storageKey` / current-origin `url` so callers
 * never `fetch()` a dead blob or a previous-launch port.
 */
export function canvasNodeReferenceSource(node: CanvasNodeData | undefined | null): {
    dataUrl: string;
    url?: string;
    storageKey?: string;
} | null {
    if (!node) return null;
    const content = node.metadata?.content?.trim() || "";
    const displayUrl = canvasNodeDisplayUrl(node);
    const mediaId = canvasNodeMediaId(node);
    const storageKey = node.metadata?.storageKey?.trim() || (mediaId ? resourceStorageKey(mediaId) : "");
    const isInline = /^data:/i.test(content);
    if (!isInline && !displayUrl && !storageKey) return null;
    return {
        dataUrl: isInline ? content : "",
        url: isInline ? undefined : displayUrl || undefined,
        storageKey: storageKey || undefined,
    };
}

type CanvasAssetUrlSource = {
    kind?: string;
    coverUrl?: string;
    data?: {
        storageKey?: string;
        dataUrl?: string;
        url?: string;
    };
};

/**
 * Live object URLs from this session's hydrate (`resolveImageUrl`) still paint.
 * Persisted `blob:` leftovers do not — callers should prefer `storageKey`.
 */
export function usableCanvasSessionUrl(path: string | null | undefined): string {
    const trimmed = path?.trim() || "";
    if (!trimmed) return "";
    if (trimmed.startsWith("blob:")) return trimmed;
    return rewriteCanvasDisplayUrl(trimmed);
}

export function canvasAssetMediaId(asset: CanvasAssetUrlSource | undefined | null): string | null {
    if (!asset) return null;
    const fromKey = resourceIdFromStorageKey(asset.data?.storageKey);
    if (fromKey) return fromKey;
    for (const candidate of [asset.data?.dataUrl, asset.data?.url, asset.coverUrl]) {
        const trimmed = candidate?.trim() || "";
        if (!trimmed) continue;
        if (trimmed.startsWith("resource:")) {
            const id = resourceIdFromStorageKey(trimmed);
            if (id) return id;
        }
        const mediaId = extractMediaIdFromCanvasMediaUrl(trimmed);
        if (mediaId) return mediaId;
    }
    return null;
}

/**
 * Paint src for a library asset. Prefers the current `/api/video-canvas/media/{id}`
 * over a stale `coverUrl` (dead blob / old desktop port / Vite-relative path).
 */
export function canvasAssetDisplayUrl(asset: CanvasAssetUrlSource | undefined | null): string {
    if (!asset) return "";
    const mediaId = canvasAssetMediaId(asset);
    if (mediaId) return canvasMediaUrl(mediaId);
    if (asset.kind === "image") {
        return usableCanvasSessionUrl(asset.data?.dataUrl) || usableCanvasSessionUrl(asset.coverUrl);
    }
    return usableCanvasSessionUrl(asset.data?.url) || usableCanvasSessionUrl(asset.coverUrl);
}
