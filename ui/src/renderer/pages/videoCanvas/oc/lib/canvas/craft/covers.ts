import type { CanvasStyleCover } from "@oc/lib/canvas/canvas-style-system";

function publicCraftBase(): string {
    const base = import.meta.env.BASE_URL || "/";
    return base.endsWith("/") ? base : `${base}/`;
}

/** Dedicated craft stills. Do not reuse Look webps — those are project style, not shot craft. */
export function craftStillUrl(coverId: string): string {
    return `${publicCraftBase()}craft/${coverId}.png`;
}

export function craftCover(coverId: string): CanvasStyleCover {
    return { from: "#1c1917", via: "#44403c", to: "#d6d3d1", image: craftStillUrl(coverId) };
}
