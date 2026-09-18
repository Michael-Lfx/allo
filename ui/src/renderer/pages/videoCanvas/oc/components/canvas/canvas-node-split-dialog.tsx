import { useEffect, useState } from "react";
import { Grid2x2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { readImageMeta } from "@oc/lib/image-utils";
import type { ImageSplitParams } from "@oc/lib/canvas/canvas-image-data";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { CanvasRange, CanvasSheet, CanvasSheetButton } from "./canvas-overlay";

export type CanvasImageSplitParams = ImageSplitParams;

const defaultParams: CanvasImageSplitParams = { rows: 2, columns: 2 };
const maxGridSize = 12;

export function CanvasNodeSplitDialog({ dataUrl, open, onClose, onConfirm }: { dataUrl: string; open: boolean; onClose: () => void; onConfirm: (params: CanvasImageSplitParams) => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [params, setParams] = useState(defaultParams);
    const [image, setImage] = useState<{ width: number; height: number } | null>(null);
    const total = params.rows * params.columns;
    const pieceSize = image ? { width: Math.max(1, Math.floor(image.width / params.columns)), height: Math.max(1, Math.floor(image.height / params.rows)) } : null;

    useEffect(() => {
        if (!open) return;
        setParams(defaultParams);
        setImage(null);
    }, [dataUrl, open]);

    useEffect(() => {
        if (!open) return;
        void readImageMeta(dataUrl).then(setImage);
    }, [dataUrl, open]);

    const update = (key: keyof CanvasImageSplitParams, value: string | number | null) => {
        setParams((current) => ({ ...current, [key]: clampGrid(value ?? current[key]) }));
    };

    return (
        <CanvasSheet
            open={open && Boolean(dataUrl)}
            theme={theme}
            width="min(780px, 94vw)"
            title={canvasT("videoCanvas.dialog.splitTitle", "切分图片")}
            subtitle={canvasT("videoCanvas.dialog.splitHint", "生成 {{total}} 个图片子节点，并按原图网格排列到画布右侧", { total })}
            onClose={onClose}
            footer={
                <CanvasSheetButton theme={theme} variant="primary" className="ml-auto" onClick={() => onConfirm(params)}>
                    <Grid2x2 className="size-3.5" />
                    {canvasT("videoCanvas.dialog.splitGenerate", "生成子节点")}
                </CanvasSheetButton>
            }
        >
            <div className="grid gap-6 md:grid-cols-[minmax(260px,1fr)_240px]">
                <div className="rounded-[var(--r-lg)] border p-4" style={{ borderColor: theme.toolbar.border }}>
                    <div className="grid min-h-[280px] place-items-center rounded-lg" style={{ background: theme.node.fill }}>
                        <div className="relative inline-block max-w-full overflow-hidden rounded-lg bg-black">
                            <img src={dataUrl} alt="" className="block max-h-[340px] max-w-full object-contain opacity-95" draggable={false} />
                            <SplitGrid rows={params.rows} columns={params.columns} />
                        </div>
                    </div>
                    <div className="mt-3 flex items-center justify-between text-sm">
                        <span style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.splitSource", "原图")}</span>
                        <span className="font-semibold">{image ? `${image.width} x ${image.height} px` : canvasT("videoCanvas.dialog.splitReading", "读取中")}</span>
                    </div>
                </div>
                <div className="space-y-5 py-1">
                    <NumberField label={canvasT("videoCanvas.dialog.splitRows", "行数")} value={params.rows} theme={theme} onChange={(value) => update("rows", value)} />
                    <NumberField label={canvasT("videoCanvas.dialog.splitCols", "列数")} value={params.columns} theme={theme} onChange={(value) => update("columns", value)} />
                    <div className="rounded-[var(--r-lg)] border px-4 py-3 text-sm" style={{ borderColor: theme.toolbar.border }}>
                        <div className="flex items-center justify-between">
                            <span style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.splitChildren", "子节点")}</span>
                            <span className="font-semibold">{canvasT("videoCanvas.dialog.splitChildCount", "{{total}} 个", { total })}</span>
                        </div>
                        <div className="mt-2 flex items-center justify-between">
                            <span style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.splitPieceApprox", "单块约")}</span>
                            <span className="font-semibold">{pieceSize ? `${pieceSize.width} x ${pieceSize.height}` : canvasT("videoCanvas.dialog.cropUnknown", "未知")}</span>
                        </div>
                    </div>
                </div>
            </div>
        </CanvasSheet>
    );
}

function NumberField({ label, value, theme, onChange }: { label: string; value: number; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onChange: (value: number) => void }) {
    return (
        <label className="block space-y-2">
            <span className="flex items-center justify-between text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.muted }}>
                {label}
                <span className="tabular-nums">{value}</span>
            </span>
            <CanvasRange theme={theme} min={1} max={maxGridSize} value={value} ariaLabel={label} onChange={onChange} />
        </label>
    );
}

function SplitGrid({ rows, columns }: CanvasImageSplitParams) {
    return (
        <div className="pointer-events-none absolute inset-0">
            {Array.from({ length: columns - 1 }).map((_, index) => (
                <div key={`column-${index}`} className="absolute inset-y-0 border-l border-white/90 shadow-[0_0_0_1px_rgba(0,0,0,.35)]" style={{ left: `${((index + 1) / columns) * 100}%` }} />
            ))}
            {Array.from({ length: rows - 1 }).map((_, index) => (
                <div key={`row-${index}`} className="absolute inset-x-0 border-t border-white/90 shadow-[0_0_0_1px_rgba(0,0,0,.35)]" style={{ top: `${((index + 1) / rows) * 100}%` }} />
            ))}
        </div>
    );
}

function clampGrid(value: string | number) {
    const numberValue = Number(value);
    return Math.min(maxGridSize, Math.max(1, Math.round(Number.isFinite(numberValue) ? numberValue : 1)));
}
