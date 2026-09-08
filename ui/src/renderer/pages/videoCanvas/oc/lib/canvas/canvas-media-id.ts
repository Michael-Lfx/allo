import { resourceIdFromStorageKey, resourceStorageKey } from "@oc/services/api/resources";
import { canvasMediaPath, canvasMediaUrl, extractMediaIdFromCanvasMediaUrl, resolveCanvasUrl } from "@renderer/pages/videoCanvas/api";
import { CanvasNodeType, type CanvasNodeData, type CanvasNodeMetadata } from "@oc/types/canvas";

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
    const content = node.metadata?.content?.trim() || "";
    // Hydrate restores IndexedDB bytes as a live object URL. Drop only orphan blobs
    // that have no durable key — those cannot be reconstituted after reload.
    if (content.startsWith("blob:") && isLocalIndexedDbStorageKey(node.metadata?.storageKey)) return content;
    return rewriteCanvasDisplayUrl(content);
}

const LOCAL_INDEXED_DB_KEY = /^(image|video|audio|file|director-clay|model):/;

/** IndexedDB object-store keys from the pre-canvas-media fallback. Not `resource:{id}`. */
export function isLocalIndexedDbStorageKey(storageKey?: string | null): boolean {
    const key = storageKey?.trim() || "";
    return LOCAL_INDEXED_DB_KEY.test(key) && !resourceIdFromStorageKey(key);
}

function isLoopbackHostname(hostname: string) {
    return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "[::1]" || hostname === "::1";
}

/**
 * Session-only bytes that vanish after reload / share unless ingested into
 * `/api/video-canvas/media/{id}`: blob URLs, data URLs, and loopback HTTP
 * that is not already a canvas media route (e.g. a local static file server).
 */
export function isEphemeralLocalMediaSrc(value?: string | null): boolean {
    const trimmed = value?.trim() || "";
    if (!trimmed) return false;
    if (trimmed.startsWith("blob:") || /^data:/i.test(trimmed)) return true;
    if (resourceIdFromStorageKey(trimmed) || extractMediaIdFromCanvasMediaUrl(trimmed)) return false;
    if (!/^https?:\/\//i.test(trimmed)) return false;
    try {
        return isLoopbackHostname(new URL(trimmed).hostname);
    } catch {
        return false;
    }
}

export function persistableCanvasMediaPath(mediaId: string): string {
    return canvasMediaPath(mediaId);
}

function persistablePreview(preview: NonNullable<CanvasNodeMetadata["videoPreview"]>): NonNullable<CanvasNodeMetadata["videoPreview"]> {
    const mediaId = resourceIdFromStorageKey(preview.storageKey) || extractMediaIdFromCanvasMediaUrl(preview.content) || "";
    if (mediaId) {
        const content = persistableCanvasMediaPath(mediaId);
        const storageKey = resourceStorageKey(mediaId);
        if (preview.content === content && preview.storageKey === storageKey) return preview;
        return { ...preview, content, storageKey };
    }
    if (preview.content?.startsWith("blob:")) return { ...preview, content: "" };
    return preview;
}

/** Drop one-shot blob URLs and rewrite canvas media onto a portable relative path. */
export function persistableCanvasNodeMetadata(metadata?: CanvasNodeMetadata): CanvasNodeMetadata | undefined {
    if (!metadata) return metadata;
    const mediaId = metadata.mediaId?.trim() || resourceIdFromStorageKey(metadata.storageKey) || extractMediaIdFromCanvasMediaUrl(metadata.content) || "";
    let next = metadata;
    const assign = (patch: Partial<CanvasNodeMetadata>) => {
        next = next === metadata ? { ...metadata, ...patch } : { ...next, ...patch };
    };
    if (mediaId) {
        const content = persistableCanvasMediaPath(mediaId);
        const storageKey = resourceStorageKey(mediaId);
        if (metadata.mediaId !== mediaId || metadata.storageKey !== storageKey || metadata.content !== content) {
            assign({ mediaId, storageKey, content });
        }
    } else {
        if (metadata.content?.startsWith("blob:")) assign({ content: "" });
        if (metadata.storageKey && isEphemeralLocalMediaSrc(metadata.storageKey) && !isLocalIndexedDbStorageKey(metadata.storageKey)) {
            assign({ storageKey: undefined });
        }
    }
    if (metadata.videoPreview) {
        const videoPreview = persistablePreview(next.videoPreview || metadata.videoPreview);
        if (videoPreview !== (next.videoPreview || metadata.videoPreview)) assign({ videoPreview });
    }
    return next;
}

export function persistableCanvasNode(node: CanvasNodeData): CanvasNodeData {
    if (node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video && node.type !== CanvasNodeType.Audio) return node;
    const metadata = persistableCanvasNodeMetadata(node.metadata);
    return metadata === node.metadata ? node : { ...node, metadata };
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
