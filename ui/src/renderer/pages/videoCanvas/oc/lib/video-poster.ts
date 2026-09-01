export type CapturedVideoPoster = {
    width: number;
    height: number;
    durationMs?: number;
    hasAudio?: boolean;
    poster?: Blob;
};

type CaptureVideoPosterOptions = {
    signal?: AbortSignal;
    maxWidth?: number;
    timeoutMs?: number;
};

const MAX_MP4_AUDIO_PROBE_BYTES = 32 * 1024 * 1024;
const MP4_AUDIO_PROBE_RANGE_BYTES = 1024 * 1024;

const videoAudioProbeCache = new Map<string, Promise<boolean | undefined>>();

let captureQueue: Promise<void> = Promise.resolve();

/** Serializes browser video decoding so batch uploads and poster hydration never fan out decoders. */
export function captureVideoPoster(source: string, options: CaptureVideoPosterOptions = {}) {
    const capture = captureQueue.then(() => captureVideoPosterNow(source, options));
    captureQueue = capture.then(() => undefined, () => undefined);
    return capture;
}

function captureVideoPosterNow(source: string, options: CaptureVideoPosterOptions) {
    return new Promise<CapturedVideoPoster>((resolve, reject) => {
        const signal = options.signal;
        if (signal?.aborted) {
            reject(abortError());
            return;
        }

        const video = document.createElement("video");
        const fallback = { width: 1280, height: 720 };
        let settled = false;
        let metadata: Omit<CapturedVideoPoster, "poster"> = fallback;

        const cleanup = () => {
            window.clearTimeout(timeoutId);
            signal?.removeEventListener("abort", handleAbort);
            video.onloadedmetadata = null;
            video.onloadeddata = null;
            video.onerror = null;
            video.pause();
            video.removeAttribute("src");
            video.load();
        };
        const finish = (result: CapturedVideoPoster) => {
            if (settled) return;
            settled = true;
            cleanup();
            resolve(result);
        };
        const fail = (error: Error) => {
            if (settled) return;
            settled = true;
            cleanup();
            reject(error);
        };
        const readMetadata = () => {
            metadata = {
                width: video.videoWidth || fallback.width,
                height: video.videoHeight || fallback.height,
                durationMs: Number.isFinite(video.duration) ? Math.round(video.duration * 1000) : undefined,
                hasAudio: detectVideoAudioTrack(video),
            };
        };
        const handleAbort = () => fail(abortError());
        const timeoutId = window.setTimeout(() => finish(metadata), options.timeoutMs ?? 15_000);

        signal?.addEventListener("abort", handleAbort, { once: true });
        video.preload = "auto";
        video.muted = true;
        video.playsInline = true;
        if (isCrossOriginHttpUrl(source)) video.crossOrigin = "anonymous";
        video.onloadedmetadata = readMetadata;
        video.onerror = () => finish(metadata);
        video.onloadeddata = () => {
            readMetadata();
            if (signal?.aborted) {
                fail(abortError());
                return;
            }
            const maxWidth = Math.max(1, options.maxWidth ?? 400);
            const width = Math.max(1, Math.min(metadata.width, maxWidth));
            const height = Math.max(1, Math.round(width * metadata.height / Math.max(1, metadata.width)));
            const canvas = document.createElement("canvas");
            canvas.width = width;
            canvas.height = height;
            const context = canvas.getContext("2d");
            if (!context) {
                finish(metadata);
                return;
            }
            try {
                context.fillStyle = "#000";
                context.fillRect(0, 0, width, height);
                context.drawImage(video, 0, 0, width, height);
                canvas.toBlob((poster) => finish({ ...metadata, poster: poster || undefined }), "image/jpeg", 0.82);
            } catch {
                // Cross-origin videos without CORS can still expose metadata but cannot be drawn safely.
                finish(metadata);
            }
        };
        video.src = source;
        video.load();
    });
}

function isCrossOriginHttpUrl(source: string) {
    try {
        const url = new URL(source, window.location.href);
        return /^https?:$/.test(url.protocol) && url.origin !== window.location.origin;
    } catch {
        return false;
    }
}

/** Audio track inspection is not standardized across browsers. */
export function detectVideoAudioTrack(video: HTMLMediaElement): boolean | undefined {
    const tracks = (video as HTMLVideoElement & { audioTracks?: { length: number } }).audioTracks;
    if (tracks && typeof tracks.length === "number") {
        if (tracks.length > 0) return true;
    }

    const mozHasAudio = (video as HTMLVideoElement & { mozHasAudio?: boolean }).mozHasAudio;
    if (typeof mozHasAudio === "boolean") return mozHasAudio;

    const webkitAudioDecodedByteCount = (video as HTMLVideoElement & { webkitAudioDecodedByteCount?: number }).webkitAudioDecodedByteCount;
    if (typeof webkitAudioDecodedByteCount === "number" && video.readyState >= 3 && webkitAudioDecodedByteCount > 0) return true;

    const captureStream = (video as HTMLVideoElement & { captureStream?: () => MediaStream }).captureStream;
    if (typeof captureStream === "function") {
        try {
            const audioTracks = captureStream.call(video).getAudioTracks();
            if (audioTracks.length > 0) return true;
        } catch {
            // Some browsers expose captureStream but reject it for remote media.
        }
    }
    return undefined;
}

export async function detectVideoAudioTrackFromBlob(blob: Blob): Promise<boolean | undefined> {
    if (!isIsoBmffVideoType(blob.type) || blob.size <= 0 || blob.size > MAX_MP4_AUDIO_PROBE_BYTES) return undefined;
    try {
        const bytes = new Uint8Array(await blob.arrayBuffer());
        return parseIsoBmffAudioTrack(bytes);
    } catch {
        return undefined;
    }
}

/**
 * Best-effort MP4/MOV header probe. Callers should prefer blob URLs already
 * fetched with Flowy auth; raw Range fetch is only used when the source is a
 * same-origin or CORS-open HTTP URL.
 */
export function detectVideoAudioTrackFromUrl(source: string, signal?: AbortSignal) {
    const normalized = source.trim();
    if (!normalized || videoAudioProbeCache.has(normalized)) return videoAudioProbeCache.get(normalized) || Promise.resolve(undefined);
    const probe = probeVideoAudioUrl(normalized, signal)
        .then((result) => {
            if (result === undefined) videoAudioProbeCache.delete(normalized);
            return result;
        })
        .catch(() => {
            videoAudioProbeCache.delete(normalized);
            return undefined;
        });
    videoAudioProbeCache.set(normalized, probe);
    return probe;
}

async function probeVideoAudioUrl(source: string, signal?: AbortSignal): Promise<boolean | undefined> {
    if (!isLikelyIsoBmffVideoUrl(source)) return undefined;
    const first = await fetchVideoRange(source, 0, MP4_AUDIO_PROBE_RANGE_BYTES - 1, signal);
    if (!first) return undefined;
    const firstResult = parseIsoBmffAudioTrack(first.bytes);
    if (firstResult !== undefined) return firstResult;

    const totalBytes = first.totalBytes;
    if (totalBytes && totalBytes > MP4_AUDIO_PROBE_RANGE_BYTES) {
        const tailStart = Math.max(0, totalBytes - MP4_AUDIO_PROBE_RANGE_BYTES);
        const tail = await fetchVideoRange(source, tailStart, totalBytes - 1, signal);
        const tailResult = tail ? parseIsoBmffAudioTrack(tail.bytes) : undefined;
        if (tailResult !== undefined) return tailResult;
    }

    if (first.status === 200 && first.bytes.byteLength <= MAX_MP4_AUDIO_PROBE_BYTES) return parseIsoBmffAudioTrack(first.bytes);
    return undefined;
}

async function fetchVideoRange(source: string, start: number, end: number, signal?: AbortSignal) {
    const response = await fetch(source, { headers: { Range: `bytes=${start}-${end}` }, signal, cache: "force-cache", credentials: "omit" });
    if (!response.ok) return undefined;
    const declaredLength = Number(response.headers.get("content-length"));
    if (Number.isFinite(declaredLength) && (response.status === 200 && declaredLength > MAX_MP4_AUDIO_PROBE_BYTES || response.status === 206 && declaredLength > MP4_AUDIO_PROBE_RANGE_BYTES)) return undefined;
    const bytes = new Uint8Array(await response.arrayBuffer());
    const contentRange = response.headers.get("content-range") || "";
    const rangeMatch = /bytes\s+\d+-\d+\/(\d+)/i.exec(contentRange);
    const totalBytes = rangeMatch ? Number(rangeMatch[1]) : Number.isFinite(declaredLength) ? declaredLength : undefined;
    return { bytes, status: response.status, totalBytes };
}

function isIsoBmffVideoType(mimeType: string) {
    const normalized = mimeType.toLowerCase().split(";", 1)[0].trim();
    return normalized === "video/mp4" || normalized === "video/quicktime" || normalized === "application/mp4";
}

function isLikelyIsoBmffVideoUrl(source: string) {
    try {
        const baseUrl = typeof window === "undefined" ? "http://localhost" : window.location?.href || "http://localhost";
        const url = new URL(source, baseUrl);
        return /\.(mp4|m4v|mov)(?:$|[?#])/i.test(url.pathname + url.search);
    } catch {
        return false;
    }
}

function parseIsoBmffAudioTrack(bytes: Uint8Array): boolean | undefined {
    const direct = parseIsoBmffAudioTrackFromOffset(bytes, 0);
    if (direct !== undefined) return direct;
    for (let offset = 0; offset + 8 <= bytes.byteLength; offset += 1) {
        if (readFourcc(bytes, offset + 4) !== "moov") continue;
        const result = parseIsoBmffAudioTrackFromOffset(bytes, offset);
        if (result !== undefined) return result;
    }
    return undefined;
}

function parseIsoBmffAudioTrackFromOffset(bytes: Uint8Array, startOffset: number): boolean | undefined {
    let foundMoov = false;
    let foundAudio = false;

    const visit = (start: number, end: number, insideMoov: boolean) => {
        let offset = start;
        while (offset + 8 <= end) {
            const size32 = readUint32(bytes, offset);
            const type = readFourcc(bytes, offset + 4);
            let headerSize = 8;
            let boxEnd: number;
            if (size32 === 1) {
                if (offset + 16 > end) return;
                const high = readUint32(bytes, offset + 8);
                const low = readUint32(bytes, offset + 12);
                const size = high * 0x100000000 + low;
                headerSize = 16;
                boxEnd = offset + size;
            } else if (size32 === 0) {
                boxEnd = end;
            } else {
                boxEnd = offset + size32;
            }
            if (!type || boxEnd < offset + headerSize || boxEnd > end) return;

            const isMoov = type === "moov";
            if (isMoov) foundMoov = true;
            if (insideMoov && type === "hdlr" && offset + 16 <= boxEnd && readFourcc(bytes, offset + 16) === "soun") foundAudio = true;

            if (isMoov || insideMoov && (type === "trak" || type === "mdia")) {
                visit(offset + headerSize, boxEnd, insideMoov || isMoov);
            }
            if (foundAudio) return;
            offset = boxEnd;
        }
    };

    visit(startOffset, bytes.byteLength, false);
    return foundMoov ? foundAudio : undefined;
}

function readUint32(bytes: Uint8Array, offset: number) {
    return ((bytes[offset] || 0) * 0x1000000) + ((bytes[offset + 1] || 0) << 16) + ((bytes[offset + 2] || 0) << 8) + (bytes[offset + 3] || 0);
}

function readFourcc(bytes: Uint8Array, offset: number) {
    if (offset + 4 > bytes.byteLength) return "";
    return String.fromCharCode(bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]);
}

function abortError() {
    return new DOMException("Video poster capture aborted", "AbortError");
}
