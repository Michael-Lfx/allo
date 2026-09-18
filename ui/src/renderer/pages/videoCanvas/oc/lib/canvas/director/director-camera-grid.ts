import type { DirectorScene, DirectorShot, DirectorViewFraming } from "@oc/types/director";
import { resolveDirectorViewFraming } from "@oc/lib/canvas/director/director-view-modes";

export type DirectorCameraGridOutput = {
    scene: DirectorScene;
    blob: Blob;
};

export type DirectorGridShape = {
    cols: number;
    rows: number;
};

/** 1 格不成宫格；4 用 2×2，9 用 3×3，其余用 5 列（LibTV 9/25 宫格）。 */
export function directorGridShape(count: number): DirectorGridShape {
    const cells = Math.max(0, Math.floor(count));
    if (cells <= 1) return { cols: 1, rows: 1 };
    if (cells <= 4) return { cols: 2, rows: Math.ceil(cells / 2) };
    if (cells <= 9) return { cols: 3, rows: Math.ceil(cells / 3) };
    return { cols: 5, rows: Math.ceil(cells / 5) };
}

export function directorGridCellRect(index: number, shape: DirectorGridShape, cell: number) {
    const col = index % shape.cols;
    const row = Math.floor(index / shape.cols);
    return { x: col * cell, y: row * cell, width: cell, height: cell };
}

/** 每个镜头一格。同一台摄影机的多条 shot 仍分格，因为构图时长/运镜不同。 */
export function listDirectorGridShots(scene: DirectorScene): DirectorShot[] {
    return scene.shots.filter((shot) => scene.cameras.some((camera) => camera.id === shot.cameraId));
}

export function resolveDirectorShotFraming(scene: DirectorScene, shotId: string, playhead = 0): DirectorViewFraming | null {
    return resolveDirectorViewFraming({ scene: { ...scene, activeShotId: shotId }, mode: "camera", playhead });
}

function drawContained(ctx: CanvasRenderingContext2D, image: CanvasImageSource, x: number, y: number, width: number, height: number) {
    const sourceWidth = "naturalWidth" in image && image.naturalWidth ? image.naturalWidth : "width" in image ? Number(image.width) : width;
    const sourceHeight = "naturalHeight" in image && image.naturalHeight ? image.naturalHeight : "height" in image ? Number(image.height) : height;
    const scale = Math.min(width / Math.max(1, sourceWidth), height / Math.max(1, sourceHeight));
    const drawWidth = sourceWidth * scale;
    const drawHeight = sourceHeight * scale;
    ctx.drawImage(image, x + (width - drawWidth) / 2, y + (height - drawHeight) / 2, drawWidth, drawHeight);
}

/**
 * 把各机位截图拼成带镜号的宫格。只在有 document 的环境跑（导演台工作台）。
 */
export async function composeDirectorCameraGrid(frames: Array<{ blob: Blob; label: string }>, cell = 512): Promise<Blob> {
    if (!frames.length) throw new Error("没有可导出的机位");
    if (typeof document === "undefined") throw new Error("宫格导出需要浏览器画布");
    const images = await Promise.all(frames.map((frame) => blobToImage(frame.blob)));
    const shape = directorGridShape(frames.length);
    const canvas = document.createElement("canvas");
    canvas.width = shape.cols * cell;
    canvas.height = shape.rows * cell;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("无法创建宫格画布");
    ctx.fillStyle = "#111418";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    frames.forEach((frame, index) => {
        const rect = directorGridCellRect(index, shape, cell);
        ctx.fillStyle = "#1c2128";
        ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
        drawContained(ctx, images[index], rect.x, rect.y, rect.width, rect.height);
        ctx.fillStyle = "rgba(0,0,0,0.55)";
        ctx.fillRect(rect.x, rect.y, Math.min(rect.width, 72), 28);
        ctx.fillStyle = "#ffffff";
        ctx.font = "600 14px ui-sans-serif, system-ui, sans-serif";
        ctx.textBaseline = "middle";
        ctx.fillText(`${index + 1}`, rect.x + 10, rect.y + 14);
        ctx.fillStyle = "rgba(255,255,255,0.88)";
        ctx.font = "12px ui-sans-serif, system-ui, sans-serif";
        ctx.fillText(frame.label.slice(0, 16), rect.x + 24, rect.y + 14);
    });
    return new Promise((resolve, reject) => {
        canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("宫格图片编码失败"))), "image/png");
    });
}

function blobToImage(blob: Blob): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
        const url = URL.createObjectURL(blob);
        const image = new Image();
        image.onload = () => {
            URL.revokeObjectURL(url);
            resolve(image);
        };
        image.onerror = () => {
            URL.revokeObjectURL(url);
            reject(new Error("机位截图无法解码"));
        };
        image.src = url;
    });
}
