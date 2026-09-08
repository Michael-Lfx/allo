import type { ViewportTransform } from "@oc/types/canvas";

/** 世界空间中一格的逻辑边长；屏幕间距另有下限，避免远距缩成灰雾。 */
export const CANVAS_GRID_WORLD_STEP = 48;
export const CANVAS_GRID_MAJOR_EVERY = 4;
export const CANVAS_GRID_MIN_SCREEN_STEP = 24;
export const CANVAS_DOT_MIN_SCREEN_STEP = 28;

type GridViewport = Pick<ViewportTransform, "x" | "y" | "k">;

export function canvasGridDevicePixelRatio(): number {
    return typeof window === "undefined" ? 1 : Math.max(1, window.devicePixelRatio || 1);
}

/** 线网在屏幕上的最小/实际间距。 */
export function canvasGridScreenStep(scale: number): number {
    return Math.max(CANVAS_GRID_WORLD_STEP * scale, CANVAS_GRID_MIN_SCREEN_STEP);
}

export function canvasGridMajorSize(minorSize: number): number {
    return minorSize * CANVAS_GRID_MAJOR_EVERY;
}

/** 点阵在缩小时不再无限压缩到屏幕像素，避免密集视觉噪声。 */
export function canvasDotGridSizePx(scale: number): number {
    return Math.max(CANVAS_GRID_WORLD_STEP * scale, CANVAS_DOT_MIN_SCREEN_STEP);
}

export function canvasDotCoreRadiusPx(scale: number): number {
    if (scale < 0.16) return 0.95;
    if (scale > 1.15) return 1.32;
    return 1.12;
}

export function canvasDotFeatherRadiusPx(scale: number): number {
    return canvasDotCoreRadiusPx(scale) + 1.18;
}

/** 空间网格点模式的点半径。远距时略小，近距时略大，始终带软边缘。 */
export function canvasDotSizePx(scale: number): string {
    return `${canvasDotCoreRadiusPx(scale)}px`;
}

export function canvasDotFeatherSizePx(scale: number): string {
    return `${canvasDotFeatherRadiusPx(scale)}px`;
}

/** 1x 用 1px 发丝；Retina 用 0.5px，避免 3x 上细到消失。 */
export function canvasGridHairlinePx(devicePixelRatio = 1): number {
    return Math.max(0.5, 1 / Math.max(1, devicePixelRatio));
}

export function canvasDotsBackgroundImage(color: string): string {
    return `radial-gradient(circle, ${color} 0px, ${color} var(--canvas-dot-size), transparent var(--canvas-dot-feather))`;
}

export function canvasLinesBackgroundImage(minorColor: string, majorColor: string): string {
    const hair = "var(--canvas-grid-hairline)";
    return [
        `linear-gradient(${majorColor} 0px, ${majorColor} ${hair}, transparent ${hair})`,
        `linear-gradient(90deg, ${majorColor} 0px, ${majorColor} ${hair}, transparent ${hair})`,
        `linear-gradient(${minorColor} 0px, ${minorColor} ${hair}, transparent ${hair})`,
        `linear-gradient(90deg, ${minorColor} 0px, ${minorColor} ${hair}, transparent ${hair})`,
    ].join(", ");
}

export function canvasGridBackgroundSize(mode: "dots" | "lines"): string {
    if (mode === "dots") return "var(--canvas-dot-grid-size) var(--canvas-dot-grid-size)";
    return [
        "var(--canvas-grid-major-size) var(--canvas-grid-major-size)",
        "var(--canvas-grid-major-size) var(--canvas-grid-major-size)",
        "var(--canvas-grid-size) var(--canvas-grid-size)",
        "var(--canvas-grid-size) var(--canvas-grid-size)",
    ].join(", ");
}

export function canvasSpatialGridCssVars(
    viewport: GridViewport,
    devicePixelRatio = 1,
): Record<string, string> {
    const gridSize = canvasGridScreenStep(viewport.k);
    const majorSize = canvasGridMajorSize(gridSize);
    const dotGridSize = canvasDotGridSizePx(viewport.k);
    return {
        "--canvas-grid-size": `${gridSize}px`,
        "--canvas-grid-x": `${viewport.x % majorSize}px`,
        "--canvas-grid-y": `${viewport.y % majorSize}px`,
        "--canvas-grid-major-size": `${majorSize}px`,
        "--canvas-grid-hairline": `${canvasGridHairlinePx(devicePixelRatio)}px`,
        "--canvas-dot-grid-size": `${dotGridSize}px`,
        "--canvas-dot-grid-x": `${viewport.x % dotGridSize}px`,
        "--canvas-dot-grid-y": `${viewport.y % dotGridSize}px`,
        "--canvas-dot-size": canvasDotSizePx(viewport.k),
        "--canvas-dot-feather": canvasDotFeatherSizePx(viewport.k),
    };
}

export function applyCanvasSpatialGridVars(
    target: HTMLElement,
    viewport: GridViewport,
    devicePixelRatio = canvasGridDevicePixelRatio(),
) {
    const vars = canvasSpatialGridCssVars(viewport, devicePixelRatio);
    for (const [name, value] of Object.entries(vars)) {
        target.style.setProperty(name, value);
    }
}
