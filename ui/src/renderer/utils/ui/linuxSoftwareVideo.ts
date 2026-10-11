import i18n from 'i18next';

import { needsWebKitSoftwareRaster } from '../platform';

/**
 * Linux desktop software-raster workaround for `<video>` playback.
 *
 * On the Linux desktop shell (WebKitGTK), when the host disables GPU
 * compositing / DMA-BUF (Houmo aarch64 AppImage sets
 * `WEBKIT_DISABLE_COMPOSITING_MODE=1`; `apps/desktop/src/main.rs` defaults
 * `WEBKIT_DISABLE_DMABUF_RENDERER=1`), the native `<video>` GStreamer overlay is
 * no longer composited into the page and every player stays a solid black box
 * (`bg-black`), even though audio keeps playing. There is no WebM fallback in
 * the pipeline either — generated clips are almost all H.264/MP4.
 *
 * Instead of touching every `<video>` call site (the canvas Vidstack player,
 * the unselected preview, and the generation list/workspace players), this
 * installs one capture-phase listener that mirrors each playing video's frames
 * onto a Canvas2D overlay positioned over the native element. The overlay is
 * clipped by Vidstack's `[data-media-provider]` box (which is `position:
 * relative; overflow: hidden`) and takes no pointer events, so audio, controls,
 * and canvas gestures keep working on the underlying `<video>`.
 *
 * Only installed when {@link needsWebKitSoftwareRaster} is true; every other
 * runtime keeps the compositing fast path untouched.
 */

const VIDEO_ERROR_KEY = 'videoCanvas.frames.loadFailed';
const VIDEO_ERROR_FALLBACK = '视频预览加载失败，请检查素材是否仍然可用';
const MAX_OVERLAY_DEVICE_PIXEL_RATIO = 2;
/** `HTMLMediaElement.HAVE_METADATA`; inline so the constant works in unit tests. */
const HAVE_METADATA = 1;

type VideoFrameCallbackVideo = HTMLVideoElement & {
    requestVideoFrameCallback?: (callback: () => void) => number;
    cancelVideoFrameCallback?: (handle: number) => void;
};

type SoftwareVideoOverlay = {
    canvas: HTMLCanvasElement;
    context: CanvasRenderingContext2D;
    host: HTMLElement;
    frameHandle: number | null;
    rafHandle: number | null;
    message: HTMLElement | null;
    disposed: boolean;
};

let installed = false;
const overlays = new WeakMap<HTMLVideoElement, SoftwareVideoOverlay>();

/** Events that start or resume mirroring. */
const PLAY_EVENTS = ['play', 'playing', 'seeked', 'loadeddata'] as const;
/** Events that stop the per-frame loop (the last mirrored frame is kept). */
const STOP_EVENTS = ['pause', 'ended', 'waiting'] as const;
/** Events that tear the overlay down completely. */
const TEARDOWN_EVENTS = ['emptied', 'abort'] as const;

/**
 * Contain-fit rectangle for a video drawn into an overlay of the given size.
 * Mirrors `object-fit: contain` so letterboxed clips are not stretched.
 */
export function computeContainedVideoRect(
    videoWidth: number,
    videoHeight: number,
    targetWidth: number,
    targetHeight: number,
): { x: number; y: number; width: number; height: number } {
    if (videoWidth <= 0 || videoHeight <= 0 || targetWidth <= 0 || targetHeight <= 0) {
        return { x: 0, y: 0, width: Math.max(0, targetWidth), height: Math.max(0, targetHeight) };
    }
    const scale = Math.min(targetWidth / videoWidth, targetHeight / videoHeight);
    const width = videoWidth * scale;
    const height = videoHeight * scale;
    return { x: (targetWidth - width) / 2, y: (targetHeight - height) / 2, width, height };
}

function playbackErrorMessage(): string {
    const translated = i18n.t(VIDEO_ERROR_KEY, { defaultValue: VIDEO_ERROR_FALLBACK });
    return typeof translated === 'string' && translated.length > 0 && translated !== VIDEO_ERROR_KEY
        ? translated
        : VIDEO_ERROR_FALLBACK;
}

function handleMediaEvent(event: Event): void {
    const target = event.target;
    if (!(target instanceof HTMLVideoElement)) return;
    if (event.type === 'play' || event.type === 'playing') {
        attachSoftwareVideoOverlay(target);
        return;
    }
    if (event.type === 'seeked' || event.type === 'loadeddata') {
        const overlay = overlays.get(target);
        if (overlay) {
            stopFrameLoop(target, overlay);
            drawOverlayFrame(target, overlay);
            startFrameLoop(target, overlay);
        }
        return;
    }
    if (event.type === 'pause' || event.type === 'ended' || event.type === 'waiting') {
        const overlay = overlays.get(target);
        if (overlay) stopFrameLoop(target, overlay);
        return;
    }
    if (event.type === 'emptied' || event.type === 'abort') {
        teardownSoftwareVideoOverlay(target);
    }
}

function resolveOverlayHost(video: HTMLVideoElement): HTMLElement | null {
    const provider = video.closest('[data-media-provider]');
    if (provider instanceof HTMLElement) return provider;
    return video.parentElement;
}

/**
 * Place the overlay directly above the `<video>` (not appended last), so any
 * later player sibling — poster, big play button, controls — still paints over
 * it. Falls back to appending into the host when the video has no parent.
 */
function placeOverlayCanvas(video: HTMLVideoElement, canvas: HTMLCanvasElement, host: HTMLElement): void {
    const parent = video.parentElement;
    if (parent) {
        parent.insertBefore(canvas, video.nextSibling);
        return;
    }
    host.appendChild(canvas);
}

/**
 * Attach the Canvas2D mirror for a video (idempotent). Exported for the
 * capture-phase installer above and for focused unit tests.
 */
export function attachSoftwareVideoOverlay(video: HTMLVideoElement): void {
    const existing = overlays.get(video);
    if (existing) {
        startFrameLoop(video, existing);
        return;
    }
    const host = resolveOverlayHost(video);
    if (!host) return;

    const canvas = document.createElement('canvas');
    canvas.setAttribute('data-canvas-software-video-overlay', 'true');
    Object.assign(canvas.style, {
        position: 'absolute',
        inset: '0',
        width: '100%',
        height: '100%',
        pointerEvents: 'none',
    } satisfies Partial<CSSStyleDeclaration>);
    const context = canvas.getContext('2d');
    if (!context) return;
    placeOverlayCanvas(video, canvas, host);

    const overlay: SoftwareVideoOverlay = {
        canvas,
        context,
        host,
        frameHandle: null,
        rafHandle: null,
        message: null,
        disposed: false,
    };
    overlays.set(video, overlay);
    // Hide only the broken native video surface; controls and audio stay live.
    video.style.opacity = '0';
    drawOverlayFrame(video, overlay);
    startFrameLoop(video, overlay);
}

/** Whether an overlay is currently attached for this video. */
export function hasSoftwareVideoOverlay(video: HTMLVideoElement): boolean {
    return overlays.has(video);
}

function startFrameLoop(video: HTMLVideoElement, overlay: SoftwareVideoOverlay): void {
    if (overlay.disposed || overlay.frameHandle !== null || overlay.rafHandle !== null) return;
    if (!video.isConnected) {
        teardownSoftwareVideoOverlay(video);
        return;
    }
    const callbackVideo = video as VideoFrameCallbackVideo;
    if (typeof callbackVideo.requestVideoFrameCallback === 'function') {
        overlay.frameHandle = callbackVideo.requestVideoFrameCallback(() => {
            overlay.frameHandle = null;
            onFrame(video, overlay);
        });
        return;
    }
    overlay.rafHandle = window.requestAnimationFrame(() => {
        overlay.rafHandle = null;
        onFrame(video, overlay);
    });
}

function onFrame(video: HTMLVideoElement, overlay: SoftwareVideoOverlay): void {
    if (overlay.disposed) return;
    if (!video.isConnected) {
        teardownSoftwareVideoOverlay(video);
        return;
    }
    // Vidstack may replace `[data-media-provider]` children on a source change.
    if (!overlay.canvas.isConnected) placeOverlayCanvas(video, overlay.canvas, overlay.host);
    drawOverlayFrame(video, overlay);
    if (!video.paused && !video.ended) startFrameLoop(video, overlay);
}

function stopFrameLoop(video: HTMLVideoElement, overlay: SoftwareVideoOverlay): void {
    if (overlay.frameHandle !== null) {
        const callbackVideo = video as VideoFrameCallbackVideo;
        callbackVideo.cancelVideoFrameCallback?.(overlay.frameHandle);
        overlay.frameHandle = null;
    }
    if (overlay.rafHandle !== null) {
        window.cancelAnimationFrame(overlay.rafHandle);
        overlay.rafHandle = null;
    }
}

function drawOverlayFrame(video: HTMLVideoElement, overlay: SoftwareVideoOverlay): void {
    // Metadata not loaded yet: neither a drawable frame nor a decode failure.
    if (video.readyState < HAVE_METADATA) return;
    if (video.videoWidth <= 0 || video.videoHeight <= 0) {
        showDecodeError(overlay);
        return;
    }
    const cssWidth = overlay.canvas.clientWidth || overlay.host.clientWidth || video.clientWidth;
    const cssHeight = overlay.canvas.clientHeight || overlay.host.clientHeight || video.clientHeight;
    if (cssWidth <= 0 || cssHeight <= 0) return;
    const ratio = Math.min(MAX_OVERLAY_DEVICE_PIXEL_RATIO, Math.max(1, window.devicePixelRatio || 1));
    const targetWidth = Math.max(1, Math.round(cssWidth * ratio));
    const targetHeight = Math.max(1, Math.round(cssHeight * ratio));
    if (overlay.canvas.width !== targetWidth || overlay.canvas.height !== targetHeight) {
        overlay.canvas.width = targetWidth;
        overlay.canvas.height = targetHeight;
    }
    const { x, y, width, height } = computeContainedVideoRect(video.videoWidth, video.videoHeight, targetWidth, targetHeight);
    overlay.context.clearRect(0, 0, targetWidth, targetHeight);
    try {
        overlay.context.drawImage(video, x, y, width, height);
    } catch {
        // Cross-origin media without CORS cannot be drawn; the overlay stays empty.
    }
}

function showDecodeError(overlay: SoftwareVideoOverlay): void {
    if (overlay.message) return;
    const message = document.createElement('div');
    message.setAttribute('data-canvas-software-video-error', 'true');
    message.textContent = playbackErrorMessage();
    Object.assign(message.style, {
        position: 'absolute',
        inset: '0',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '16px',
        textAlign: 'center',
        color: 'rgba(255, 255, 255, 0.75)',
        fontSize: '12px',
        lineHeight: '1.4',
        pointerEvents: 'none',
    } satisfies Partial<CSSStyleDeclaration>);
    overlay.host.appendChild(message);
    overlay.message = message;
}

/**
 * Tear down the Canvas2D mirror for a video and restore its native opacity.
 * Exported for focused unit tests.
 */
export function teardownSoftwareVideoOverlay(video: HTMLVideoElement): void {
    const overlay = overlays.get(video);
    if (!overlay) return;
    overlays.delete(video);
    overlay.disposed = true;
    stopFrameLoop(video, overlay);
    overlay.canvas.remove();
    overlay.message?.remove();
    video.style.opacity = '';
}

/**
 * Install the capture-phase media listeners exactly once, and only on the Linux
 * desktop software-raster path. Safe to call unconditionally at startup.
 */
export function installLinuxSoftwareVideoOverlay(): void {
    if (installed || typeof document === 'undefined' || typeof window === 'undefined') return;
    if (!needsWebKitSoftwareRaster()) return;
    installed = true;
    for (const type of [...PLAY_EVENTS, ...STOP_EVENTS, ...TEARDOWN_EVENTS]) {
        document.addEventListener(type, handleMediaEvent, true);
    }
    // A reload/navigation drops every overlay with its element.
    window.addEventListener('pagehide', () => {
        document.querySelectorAll('canvas[data-canvas-software-video-overlay]').forEach((canvas) => canvas.remove());
    });
}

installLinuxSoftwareVideoOverlay();
