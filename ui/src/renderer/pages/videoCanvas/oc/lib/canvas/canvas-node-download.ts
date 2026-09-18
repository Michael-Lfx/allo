import { audioExtension, imageExtension } from "@oc/lib/canvas/canvas-project-generation";
import { buildCanvasMediaDownloadFileName } from "@oc/lib/canvas/canvas-media-download";
import { getMediaBlob } from "@oc/services/file-storage";
import { getImageBlob } from "@oc/services/image-storage";
import { resourceIdFromStorageKey, resourceStorageKey } from "@oc/services/api/resources";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import { buildBackendAuthHeaders } from "@/common/adapter/httpBridge";
import { resolveCanvasUrl } from "@renderer/pages/videoCanvas/api";

const EXPIRED_MEDIA_HINT = "无法下载：本地缓存缺失，且媒体链接可能已过期，请重新生成后再试";

/** Recover `resource:{id}` from a canvas media URL when metadata lost the storageKey. */
function storageKeyFromContent(content: string): string | undefined {
    const path = content.split(/[?#]/, 1)[0] || "";
    const match = path.match(/\/api\/video-canvas\/media\/([^/]+)$/);
    if (!match?.[1]) return undefined;
    try {
        return resourceStorageKey(decodeURIComponent(match[1]));
    } catch {
        return resourceStorageKey(match[1]);
    }
}

function isLikelyNetworkFetchError(error: unknown) {
    const message = error instanceof Error ? error.message : String(error || "");
    return /\b(?:failed to fetch|fetch failed|networkerror|load failed|network request failed)\b/i.test(message);
}

/** Resolve media bytes for download; never rely on bare saveAs(remoteUrl) (auth/CORS). */
export async function resolveCanvasNodeMediaBlob(node: CanvasNodeData): Promise<Blob> {
    let storageKey = node.metadata?.storageKey?.trim() || "";
    const content = node.metadata?.content?.trim() || "";

    if (!storageKey && content) {
        storageKey = storageKeyFromContent(content) || "";
    }

    if (storageKey) {
        const blob =
            node.type === CanvasNodeType.Image || storageKey.startsWith("image:")
                ? await getImageBlob(storageKey).catch(() => null)
                : await getMediaBlob(storageKey).catch(() => null);
        if (blob && blob.size > 0) return blob;
    }

    if (!content) throw new Error("当前节点没有可下载的内容");

    if (content.startsWith("data:") || content.startsWith("blob:")) {
        try {
            const response = await fetch(content);
            if (!response.ok) throw new Error("读取本地媒体失败");
            const blob = await response.blob();
            if (!blob.size) throw new Error("媒体内容为空");
            return blob;
        } catch (error) {
            if (error instanceof Error && !isLikelyNetworkFetchError(error)) throw error;
            throw new Error(EXPIRED_MEDIA_HINT);
        }
    }

    const absolute = resolveCanvasUrl(content) || content;
    try {
        const response = await fetch(absolute, {
            method: "GET",
            headers: buildBackendAuthHeaders("GET"),
            credentials: "omit",
            cache: "no-store",
        });
        if (!response.ok) {
            if (response.status === 404 || response.status === 410) throw new Error(EXPIRED_MEDIA_HINT);
            throw new Error(`下载失败（${response.status}）`);
        }
        const blob = await response.blob();
        if (!blob.size) throw new Error("下载到的文件为空");
        return blob;
    } catch (error) {
        if (error instanceof Error && error.message === EXPIRED_MEDIA_HINT) throw error;
        if (error instanceof Error && /下载失败（\d+）/.test(error.message)) throw error;
        if (storageKey && resourceIdFromStorageKey(storageKey)) throw new Error(EXPIRED_MEDIA_HINT);
        if (isLikelyNetworkFetchError(error)) throw new Error(EXPIRED_MEDIA_HINT);
        throw error instanceof Error ? error : new Error(EXPIRED_MEDIA_HINT);
    }
}

export function canvasNodeDownloadFileName(node: CanvasNodeData) {
    const base = `canvas-${node.type}-${node.id}`;
    if (node.type === CanvasNodeType.Video) {
        const ext = (node.metadata?.mimeType || "").includes("webm") ? "webm" : "mp4";
        return `${base}.${ext}`;
    }
    if (node.type === CanvasNodeType.Audio) {
        return `${base}.${audioExtension(node.metadata?.mimeType)}`;
    }
    const mime = node.metadata?.mimeType || "";
    const fromMime = mime.match(/image[/]([\w+.-]+)/)?.[1]?.replace("jpeg", "jpg");
    return `${base}.${fromMime || imageExtension(node.metadata?.content || "")}`;
}

function mimeForNode(node: CanvasNodeData) {
    if (node.metadata?.mimeType) return node.metadata.mimeType;
    if (node.type === CanvasNodeType.Video) return "video/mp4";
    if (node.type === CanvasNodeType.Audio) return "audio/mpeg";
    return "image/png";
}

function extensionForNode(node: CanvasNodeData) {
    return canvasNodeDownloadFileName(node).split(".").pop() || "bin";
}

type SaveFilePickerWindow = Window & {
    showSaveFilePicker?: (options?: {
        suggestedName?: string;
        types?: Array<{ description?: string; accept: Record<string, string[]> }>;
    }) => Promise<FileSystemFileHandle>;
};

/** Must run before long async work — browsers require transient user activation. */
async function openSaveFileHandle(node: CanvasNodeData, fileName: string): Promise<FileSystemFileHandle | null> {
    const picker = (window as SaveFilePickerWindow).showSaveFilePicker;
    if (typeof picker !== "function") return null;
    const ext = extensionForNode(node);
    const mime = mimeForNode(node);
    return picker({
        suggestedName: fileName,
        types: [
            {
                description: node.type === CanvasNodeType.Video ? "视频" : node.type === CanvasNodeType.Audio ? "音频" : "图片",
                accept: { [mime]: [`.${ext}`] },
            },
        ],
    });
}

function triggerAnchorDownload(blob: Blob, fileName: string) {
    const objectUrl = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = objectUrl;
    anchor.download = fileName;
    anchor.rel = "noopener";
    anchor.style.display = "none";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 60_000);
}

/**
 * Save node media to disk.
 * Opens the save picker first (while the click still counts as user activation),
 * then fetches bytes and writes them. Falls back to an object-URL anchor download.
 */
export async function downloadCanvasNodeMedia(node: CanvasNodeData, options?: { canvasTitle?: string }): Promise<"saved" | "triggered"> {
    if (node.type !== CanvasNodeType.Image && node.type !== CanvasNodeType.Video && node.type !== CanvasNodeType.Audio) {
        throw new Error("当前节点类型不支持下载");
    }
    const fileName = options?.canvasTitle
        ? buildCanvasMediaDownloadFileName(options.canvasTitle, node)
        : canvasNodeDownloadFileName(node);

    let fileHandle: FileSystemFileHandle | null = null;
    try {
        fileHandle = await openSaveFileHandle(node, fileName);
    } catch (error) {
        if (error instanceof DOMException && error.name === "AbortError") throw error;
        fileHandle = null;
    }

    const blob = await resolveCanvasNodeMediaBlob(node);
    const typedBlob = blob.type ? blob : new Blob([blob], { type: mimeForNode(node) });

    if (fileHandle) {
        const writable = await fileHandle.createWritable();
        try {
            await writable.write(typedBlob);
        } finally {
            await writable.close();
        }
        return "saved";
    }

    triggerAnchorDownload(typedBlob, fileName);
    return "triggered";
}
