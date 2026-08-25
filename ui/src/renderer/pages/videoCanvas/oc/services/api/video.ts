import { uploadMediaFile, resolveMediaUrl, getMediaBlob, type UploadedFile } from "@oc/services/file-storage";
import { primeResourceBlobCache } from "@oc/services/resource-blob-cache";
import { resourceIdFromStorageKey } from "@oc/services/api/resources";

export type VideoGenerationResult = {
    blob?: Blob;
    url?: string;
    storageKey?: string;
    mimeType?: string;
    width?: number;
    height?: number;
    bytes?: number;
    durationMs?: number;
};

/**
 * Persist a generated video durably.
 * - Prefer an existing canvas/backend `storageKey` (already on disk) and prime the blob cache.
 * - Otherwise upload the blob/URL into local media so downloads do not depend on short-lived cloud links.
 */
export async function storeGeneratedVideo(result: VideoGenerationResult): Promise<UploadedFile> {
    if (result.blob) return uploadMediaFile(result.blob, "video");

    const storageKey = result.storageKey?.trim();
    if (storageKey) {
        const url = (await resolveMediaUrl(storageKey, result.url || "")) || result.url || "";
        // Block until the durable blob cache is warm so later downloads do not hit expired cloud URLs.
        await warmVideoBlobCache(storageKey).catch(() => undefined);
        return {
            url,
            storageKey,
            bytes: result.bytes || 0,
            mimeType: result.mimeType || "video/mp4",
            width: result.width,
            height: result.height,
            durationMs: result.durationMs,
        };
    }

    if (result.url) {
        // Temporary cloud URLs expire in hours — always copy into durable local media.
        try {
            return await uploadMediaFile(result.url, "video");
        } catch (error) {
            const detail = error instanceof Error ? error.message : "Fetch failed";
            throw new Error(`视频未能保存到本地（${detail}）。云端链接可能已失效，请重新生成。`);
        }
    }

    throw new Error("视频接口没有返回可播放的视频");
}

async function warmVideoBlobCache(storageKey: string) {
    if (!resourceIdFromStorageKey(storageKey)) return;
    const blob = await getMediaBlob(storageKey);
    if (blob && blob.size > 0) await primeResourceBlobCache(storageKey, blob);
}
