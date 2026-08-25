export const VIDEO_DURATION_OPTIONS = [6, 9, 10, 15] as const;
export const VIDEO_RESOLUTION_OPTIONS = [480, 720, 1080, 2160] as const;
export const VIDEO_DURATION_MIN = 1;

export function normalizeVideoDuration(value: string | number | undefined) {
    const seconds = Math.floor(Number(value) || VIDEO_DURATION_OPTIONS[0]);
    return String(Math.max(VIDEO_DURATION_MIN, seconds));
}

/**
 * Legacy canvas-local normalizer used by generic (non-Seedance / non-MiniMax) UI.
 * Preserves MiniMax-H3 tokens (`768P` / `2K`) so mid-pipeline helpers do not
 * collapse them into Seedance-style numeric heights.
 *
 * Prefer {@link canonicalizeVideoResolution} when a model id is known.
 */
export function normalizeVideoResolution(value: string | number | undefined) {
    const token = String(value || "").trim();
    const lower = token.toLowerCase().replace(/[_\s]/g, "");
    if (lower === "2k") return "2K";
    if (lower === "768" || lower === "768p") return "768P";
    if (token === "low") return "480";
    if (token === "auto" || token === "medium" || token === "high") return "720";
    if (token === "4k") return "2160";
    const resolution = Number(token.replace(/p$/i, "")) || 720;
    return String(nearestOption(resolution, VIDEO_RESOLUTION_OPTIONS));
}

/** True when the token is a MiniMax-H3 resolution (not a Seedance `NNp` height). */
export function isMiniMaxH3ResolutionToken(value: string | undefined) {
    const lower = String(value || "").trim().toLowerCase().replace(/[_\s]/g, "");
    return lower === "2k" || lower === "768" || lower === "768p";
}

function nearestOption(value: number, options: readonly number[]) {
    return options.reduce((nearest, option) => Math.abs(option - value) < Math.abs(nearest - value) ? option : nearest, options[0]);
}
