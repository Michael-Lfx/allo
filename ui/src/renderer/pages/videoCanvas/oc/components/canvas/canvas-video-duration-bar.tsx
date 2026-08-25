import { useMemo, useRef } from "react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import type { CanvasTheme } from "@oc/lib/canvas-theme";

type CanvasVideoDurationBarProps = {
    value: number;
    min: number;
    max: number;
    step?: number;
    ticks?: number[];
    theme: CanvasTheme;
    onChange: (seconds: number) => void;
};

function clampDuration(secs: number, min: number, max: number, step: number) {
    if (!Number.isFinite(secs)) return min;
    const stepped = Math.round(secs / step) * step;
    return Math.min(max, Math.max(min, stepped));
}

function defaultTicks(min: number, max: number, step: number) {
    const span = Math.max(1, max - min);
    const count = Math.min(6, Math.floor(span / Math.max(step, 1)) + 1);
    if (count <= 1) return [min, max].filter((value, index, list) => list.indexOf(value) === index);
    return Array.from({ length: count }, (_, index) => clampDuration(min + (span * index) / (count - 1), min, max, step));
}

/** Compact scrubber for canvas video-node duration settings (Seedance / MiniMax / generic). */
export function CanvasVideoDurationBar({ value, min, max, step = 1, ticks, theme, onChange }: CanvasVideoDurationBarProps) {
    const scrubRef = useRef<HTMLDivElement | null>(null);
    const draggingRef = useRef(false);
    const secs = clampDuration(value, min, max, step);
    const visibleTicks = useMemo(() => {
        const source = (ticks?.length ? ticks : defaultTicks(min, max, step)).filter((tick) => tick >= min && tick <= max);
        return Array.from(new Set(source.map((tick) => clampDuration(tick, min, max, step)))).sort((a, b) => a - b);
    }, [ticks, min, max, step]);
    const progress = ((secs - min) / Math.max(1, max - min)) * 100;

    const seekFromClientX = (clientX: number) => {
        const el = scrubRef.current;
        if (!el) return;
        const rect = el.getBoundingClientRect();
        if (rect.width <= 0) return;
        const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
        onChange(clampDuration(min + ratio * (max - min), min, max, step));
    };

    return (
        <div className="space-y-2" onMouseDown={(event) => event.stopPropagation()}>
            <div className="flex items-center justify-between gap-2">
                <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>
                    {canvasT("videoCanvas.settings.durationHint", "拖动或点击刻度")}
                </span>
                <span className="tabular-nums text-[var(--fs-label)] font-semibold" style={{ color: theme.node.text }} aria-live="polite">
                    {secs}
                    <span className="ml-0.5 font-medium opacity-55">s</span>
                </span>
            </div>

            <div className="relative px-0.5 pb-4 pt-1">
                <div
                    ref={scrubRef}
                    className="relative h-7 cursor-pointer touch-none"
                    onPointerDown={(event) => {
                        event.preventDefault();
                        draggingRef.current = true;
                        event.currentTarget.setPointerCapture(event.pointerId);
                        seekFromClientX(event.clientX);
                    }}
                    onPointerMove={(event) => {
                        if (!draggingRef.current) return;
                        seekFromClientX(event.clientX);
                    }}
                    onPointerUp={(event) => {
                        if (!draggingRef.current) return;
                        draggingRef.current = false;
                        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                            event.currentTarget.releasePointerCapture(event.pointerId);
                        }
                    }}
                    onPointerCancel={() => {
                        draggingRef.current = false;
                    }}
                >
                    <div className="absolute inset-x-0 top-1/2 h-1.5 -translate-y-1/2 overflow-hidden rounded-full" style={{ background: theme.toolbar.itemHover }}>
                        <div className="h-full rounded-full" style={{ width: `${progress}%`, background: theme.toolbar.activeBg || theme.node.activeStroke }} />
                    </div>
                    <div
                        className="absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 shadow-sm"
                        style={{ left: `${progress}%`, background: theme.node.fill, borderColor: theme.node.activeStroke }}
                        aria-hidden
                    />
                    <input
                        type="range"
                        min={min}
                        max={max}
                        step={step}
                        value={secs}
                        aria-valuemin={min}
                        aria-valuemax={max}
                        aria-valuenow={secs}
                        aria-label={canvasT("videoCanvas.settings.durationAria", "视频时长（秒）")}
                        className="absolute inset-0 m-0 h-full w-full cursor-pointer opacity-0"
                        onChange={(event) => onChange(clampDuration(Number(event.target.value), min, max, step))}
                    />
                </div>

                <div className="pointer-events-none absolute inset-x-0.5 bottom-0 h-4">
                    {visibleTicks.map((tick) => {
                        const left = ((tick - min) / Math.max(1, max - min)) * 100;
                        const current = tick === secs;
                        return (
                            <button
                                key={tick}
                                type="button"
                                className="pointer-events-auto absolute top-0 -translate-x-1/2 text-[10px] leading-none transition-opacity"
                                style={{ left: `${left}%`, color: current ? theme.node.text : theme.node.muted, fontWeight: current ? 600 : 500 }}
                                onMouseDown={(event) => event.stopPropagation()}
                                onClick={() => onChange(clampDuration(tick, min, max, step))}
                                aria-label={`${tick}s`}
                                aria-pressed={current}
                            >
                                {tick}
                            </button>
                        );
                    })}
                </div>
            </div>
        </div>
    );
}
