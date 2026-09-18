import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Brush, Eraser, RotateCcw, WandSparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { readImageMeta } from "@oc/lib/image-utils";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { ChoiceChip } from "@oc/components/generation-settings-chrome";
import { CanvasRange, CanvasSheet, CanvasSheetButton } from "./canvas-overlay";

export type CanvasImageMaskEditPayload = {
    prompt: string;
    maskDataUrl: string;
};

type DrawMode = "paint" | "erase";

const defaultBrushSize = 100;
const maskFillColor = "rgba(37, 99, 235, .38)";
const maskBorderColor = "rgba(255, 255, 255, .72)";

export function CanvasNodeMaskEditDialog({ dataUrl, open, onClose, onConfirm }: { dataUrl: string; open: boolean; onClose: () => void; onConfirm: (payload: CanvasImageMaskEditPayload) => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const maskCanvasRef = useRef<HTMLCanvasElement>(null);
    const previewCanvasRef = useRef<HTMLCanvasElement>(null);
    const drawingRef = useRef<{ active: boolean; last: { x: number; y: number } | null }>({ active: false, last: null });
    const [image, setImage] = useState<{ width: number; height: number } | null>(null);
    const [prompt, setPrompt] = useState("");
    const [brushSize, setBrushSize] = useState(defaultBrushSize);
    const [mode, setMode] = useState<DrawMode>("paint");
    const [error, setError] = useState("");

    useEffect(() => {
        if (!open) return;
        setPrompt("");
        setBrushSize(defaultBrushSize);
        setMode("paint");
        setError("");
        void readImageMeta(dataUrl).then(setImage);
    }, [dataUrl, open]);

    useEffect(() => {
        clearCanvas(maskCanvasRef.current);
        clearCanvas(previewCanvasRef.current);
    }, [image]);

    const draw = (event: ReactPointerEvent<HTMLCanvasElement>) => {
        const point = readCanvasPoint(event.currentTarget, event.clientX, event.clientY);
        const maskCanvas = maskCanvasRef.current;
        const context = maskCanvas?.getContext("2d");
        if (!maskCanvas || !context) return;
        context.lineCap = "round";
        context.lineJoin = "round";
        context.lineWidth = brushSize;
        context.globalCompositeOperation = mode === "paint" ? "source-over" : "destination-out";
        context.strokeStyle = "#000";
        context.fillStyle = "#000";
        if (!drawingRef.current.last) {
            drawMaskStroke(context, point, point, brushSize);
        } else {
            drawMaskStroke(context, drawingRef.current.last, point, brushSize);
        }
        renderMaskPreview(maskCanvas, previewCanvasRef.current);
        drawingRef.current.last = point;
        if (mode === "paint") {
            setError("");
        }
    };

    const startDraw = (event: ReactPointerEvent<HTMLCanvasElement>) => {
        event.preventDefault();
        event.stopPropagation();
        event.currentTarget.setPointerCapture(event.pointerId);
        drawingRef.current = { active: true, last: null };
        if (maskCanvasRef.current) renderMaskPreview(maskCanvasRef.current, previewCanvasRef.current);
        draw(event);
    };

    const moveDraw = (event: ReactPointerEvent<HTMLCanvasElement>) => {
        if (!drawingRef.current.active) return;
        event.preventDefault();
        draw(event);
    };

    const stopDraw = () => {
        drawingRef.current = { active: false, last: null };
        const maskCanvas = maskCanvasRef.current;
        if (maskCanvas) renderMaskPreview(maskCanvas, previewCanvasRef.current, canvasHasPaint(maskCanvas));
    };

    const resetMask = () => {
        clearCanvas(maskCanvasRef.current);
        clearCanvas(previewCanvasRef.current);
        setError("");
    };

    const submit = () => {
        const nextPrompt = prompt.trim();
        const canvas = maskCanvasRef.current;
        if (!nextPrompt) return setError(canvasT("videoCanvas.mask.promptRequired", "请输入修改要求"));
        if (!canvas) return;
        if (!canvasHasPaint(canvas)) return setError(canvasT("videoCanvas.mask.maskRequired", "请先涂抹局部区域"));
        onConfirm({ prompt: nextPrompt, maskDataUrl: buildEditMask(canvas) });
    };

    return (
        <CanvasSheet
            open={open && Boolean(dataUrl)}
            theme={theme}
            width="min(980px, 94vw)"
            title={canvasT("videoCanvas.mask.title", "局部遮罩编辑")}
            subtitle={image ? `${image.width} × ${image.height}px` : canvasT("videoCanvas.mask.reading", "读取中")}
            onClose={onClose}
            footer={
                <>
                    <CanvasSheetButton theme={theme} onClick={resetMask}>
                        <RotateCcw className="size-3.5" />
                        {canvasT("videoCanvas.mask.reset", "重置")}
                    </CanvasSheetButton>
                    <span className="flex-1" />
                    <CanvasSheetButton theme={theme} onClick={onClose}>{canvasT("videoCanvas.mask.cancel", "取消")}</CanvasSheetButton>
                    <CanvasSheetButton theme={theme} variant="primary" onClick={submit}>
                        <WandSparkles className="size-3.5" />
                        {canvasT("videoCanvas.mask.apply", "AI 修改")}
                    </CanvasSheetButton>
                </>
            }
        >
            <div className="grid gap-4 lg:grid-cols-[minmax(360px,1fr)_280px]">
                <div className="flex min-h-[320px] items-center justify-center overflow-hidden rounded-[var(--r-lg)]" style={{ background: theme.node.fill }}>
                    <div className="relative inline-block max-w-full overflow-hidden rounded-lg select-none">
                        <img src={dataUrl} alt="" className="block max-h-[58vh] max-w-full" draggable={false} />
                        {image ? (
                            <>
                                <canvas ref={maskCanvasRef} width={image.width} height={image.height} className="hidden" />
                                <canvas
                                    ref={previewCanvasRef}
                                    width={image.width}
                                    height={image.height}
                                    className="absolute inset-0 h-full w-full cursor-crosshair touch-none"
                                    onPointerDown={startDraw}
                                    onPointerMove={moveDraw}
                                    onPointerUp={stopDraw}
                                    onPointerCancel={stopDraw}
                                />
                            </>
                        ) : null}
                    </div>
                </div>

                <div className="flex min-h-[280px] flex-col gap-4">
                    <div className="flex flex-wrap gap-1.5">
                        <ChoiceChip selected={mode === "paint"} theme={theme} onClick={() => setMode("paint")}>
                            <span className="inline-flex items-center gap-1"><Brush className="size-3.5" />{canvasT("videoCanvas.mask.brush", "画笔")}</span>
                        </ChoiceChip>
                        <ChoiceChip selected={mode === "erase"} theme={theme} onClick={() => setMode("erase")}>
                            <span className="inline-flex items-center gap-1"><Eraser className="size-3.5" />{canvasT("videoCanvas.mask.erase", "擦除")}</span>
                        </ChoiceChip>
                    </div>

                    <div className="space-y-1.5">
                        <div className="flex items-center justify-between text-[var(--fs-tiny)]">
                            <span style={{ color: theme.node.muted }}>{canvasT("videoCanvas.mask.brushSize", "笔刷大小")}</span>
                            <span className="font-semibold tabular-nums">{brushSize}px</span>
                        </div>
                        <CanvasRange theme={theme} min={8} max={160} step={2} value={brushSize} ariaLabel={canvasT("videoCanvas.mask.brushSize", "笔刷大小")} onChange={setBrushSize} />
                    </div>

                    <div className="space-y-1.5">
                        <div className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.mask.prompt", "修改要求")}</div>
                        <textarea
                            rows={6}
                            className="canvas-sheet-input min-h-28 w-full resize-y py-2"
                            value={prompt}
                            placeholder={canvasT("videoCanvas.mask.promptPlaceholder", "例如：把选中区域改成金属材质，保持原图光影")}
                            onChange={(event) => {
                                setPrompt(event.target.value);
                                setError("");
                            }}
                        />
                        {error ? <div className="text-xs font-medium" style={{ color: theme.accent.danger }}>{error}</div> : null}
                    </div>
                </div>
            </div>
        </CanvasSheet>
    );
}

function readCanvasPoint(canvas: HTMLCanvasElement, clientX: number, clientY: number) {
    const rect = canvas.getBoundingClientRect();
    return {
        x: ((clientX - rect.left) / Math.max(1, rect.width)) * canvas.width,
        y: ((clientY - rect.top) / Math.max(1, rect.height)) * canvas.height,
    };
}

function clearCanvas(canvas: HTMLCanvasElement | null) {
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    context.clearRect(0, 0, canvas.width, canvas.height);
}

function drawMaskStroke(context: CanvasRenderingContext2D, from: { x: number; y: number }, to: { x: number; y: number }, size: number) {
    if (from.x === to.x && from.y === to.y) {
        context.beginPath();
        context.arc(to.x, to.y, size / 2, 0, Math.PI * 2);
        context.fill();
        return;
    }
    context.beginPath();
    context.moveTo(from.x, from.y);
    context.lineTo(to.x, to.y);
    context.stroke();
}

function canvasHasPaint(canvas: HTMLCanvasElement) {
    const context = canvas.getContext("2d");
    if (!context) return false;
    const data = context.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 3; index < data.length; index += 4) {
        if (data[index] > 0) return true;
    }
    return false;
}

function renderMaskPreview(maskCanvas: HTMLCanvasElement, previewCanvas: HTMLCanvasElement | null, withBorder = false) {
    const context = previewCanvas?.getContext("2d");
    if (!previewCanvas || !context) return;
    context.clearRect(0, 0, previewCanvas.width, previewCanvas.height);
    context.fillStyle = maskFillColor;
    context.fillRect(0, 0, previewCanvas.width, previewCanvas.height);
    context.globalCompositeOperation = "destination-in";
    context.drawImage(maskCanvas, 0, 0);
    context.globalCompositeOperation = "source-over";
    if (withBorder) drawDashedMaskBorder(context, maskCanvas);
}

function drawDashedMaskBorder(context: CanvasRenderingContext2D, maskCanvas: HTMLCanvasElement) {
    const maskContext = maskCanvas.getContext("2d");
    if (!maskContext) return;
    const { width, height } = maskCanvas;
    const data = maskContext.getImageData(0, 0, width, height).data;
    const step = Math.max(1, Math.round(Math.max(width, height) / 1200));
    const dash = step * 8;
    const gap = step * 5;
    const period = dash + gap;

    context.save();
    context.fillStyle = maskBorderColor;
    context.shadowColor = "rgba(0, 0, 0, .24)";
    context.shadowBlur = step * 1.5;
    for (let y = step; y < height - step; y += step) {
        for (let x = step; x < width - step; x += step) {
            const offset = (y * width + x) * 4 + 3;
            if (data[offset] === 0 || !isMaskEdge(data, width, x, y, step)) continue;
            if ((x + y) % period > dash) continue;
            context.fillRect(x - step / 2, y - step / 2, Math.max(1.5, step), Math.max(1.5, step));
        }
    }
    context.restore();
}

function isMaskEdge(data: Uint8ClampedArray, width: number, x: number, y: number, step: number) {
    return data[((y - step) * width + x) * 4 + 3] === 0 || data[((y + step) * width + x) * 4 + 3] === 0 || data[(y * width + x - step) * 4 + 3] === 0 || data[(y * width + x + step) * 4 + 3] === 0;
}

function buildEditMask(selectionCanvas: HTMLCanvasElement) {
    const canvas = document.createElement("canvas");
    canvas.width = selectionCanvas.width;
    canvas.height = selectionCanvas.height;
    const context = canvas.getContext("2d");
    if (!context) return selectionCanvas.toDataURL("image/png");
    const selectionContext = selectionCanvas.getContext("2d");
    context.fillStyle = "#fff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    if (!selectionContext) return canvas.toDataURL("image/png");
    const selection = selectionContext.getImageData(0, 0, canvas.width, canvas.height);
    const mask = context.getImageData(0, 0, canvas.width, canvas.height);
    for (let index = 3; index < mask.data.length; index += 4) {
        if (selection.data[index] > 0) mask.data[index] = 0;
    }
    context.putImageData(mask, 0, 0);
    return canvas.toDataURL("image/png");
}
