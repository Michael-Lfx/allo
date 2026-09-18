/** Canvas can fan out N parallel tasks even when a provider only accepts n=1 per request. */
export const CANVAS_IMAGE_BATCH_MAX_COUNT = 8;
export const CANVAS_VIDEO_BATCH_MAX_COUNT = 4;
export const CANVAS_IMAGE_QUICK_COUNTS = [1, 2, 4] as const;
export const CANVAS_VIDEO_QUICK_COUNTS = [1, 2, 4] as const;

export function getCanvasBatchCount(count: string | number | undefined, max: number) {
    return Math.max(1, Math.min(max, Math.floor(Math.abs(Number(count)) || 1)));
}
