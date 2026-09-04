import { useEffect, useRef, useState, type RefObject } from "react";
import { Compass, Focus, HelpCircle, LayoutTemplate, Minus, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CanvasOverlay } from "@oc/components/canvas/canvas-overlay";
import { FloatingDock, type FloatingDockEntry } from "@oc/components/ui/aceternity/floating-dock";
import { canvasDockStyle } from "@oc/lib/canvas/canvas-aceternity-style";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { subscribeCanvasViewportPreview } from "@oc/lib/canvas/canvas-live-viewport";
import { useThemeStore } from "@oc/stores/use-theme-store";

type CanvasZoomControlsProps = {
    scale: number;
    onScaleChange: (scale: number) => void;
    onReset: () => void;
    onAutoArrange?: () => void;
    isMiniMapOpen: boolean;
    onToggleMiniMap: () => void;
    onOpenShortcuts: () => void;
    containerRef?: RefObject<HTMLDivElement | null>;
};

const QUICK_ZOOM_LEVELS = [0.25, 0.5, 1, 2] as const;

export function CanvasZoomControls({ scale, onScaleChange, onReset, onAutoArrange, isMiniMapOpen, onToggleMiniMap, onOpenShortcuts, containerRef }: CanvasZoomControlsProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const rootRef = useRef<HTMLDivElement>(null);
    const liveScaleRef = useRef(scale);
    const rangeRef = useRef<HTMLInputElement>(null);
    const dockLabelRef = useRef<HTMLSpanElement>(null);
    const panelLabelRef = useRef<HTMLSpanElement>(null);
    const [precisionOpen, setPrecisionOpen] = useState(false);
    const dockStyle = canvasDockStyle(theme);

    useEffect(() => updateScaleDisplay(scale), [scale]);

    useEffect(() => {
        const container = containerRef?.current;
        if (!container) return;
        return subscribeCanvasViewportPreview(container, (viewport) => updateScaleDisplay(viewport.k));
    }, [containerRef]);

    useEffect(() => {
        if (!precisionOpen) return;
        const close = (event: PointerEvent) => {
            if (event.target instanceof Node && !rootRef.current?.contains(event.target)) setPrecisionOpen(false);
        };
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key === "Escape") setPrecisionOpen(false);
        };
        document.addEventListener("pointerdown", close, true);
        document.addEventListener("keydown", closeOnEscape);
        return () => {
            document.removeEventListener("pointerdown", close, true);
            document.removeEventListener("keydown", closeOnEscape);
        };
    }, [precisionOpen]);

    function updateScaleDisplay(nextScale: number) {
        liveScaleRef.current = nextScale;
        const percent = String(Math.round(nextScale * 100));
        if (rangeRef.current) rangeRef.current.value = percent;
        if (dockLabelRef.current) dockLabelRef.current.textContent = percent;
        if (panelLabelRef.current) panelLabelRef.current.textContent = `${percent}%`;
    }

    function commitScale(nextScale: number) {
        const clampedScale = Math.min(2, Math.max(0.05, nextScale));
        updateScaleDisplay(clampedScale);
        onScaleChange(clampedScale);
    }

    const navigateItems: FloatingDockEntry[] = [
        {
            id: "zoom-minimap",
            label: isMiniMapOpen ? canvasT("videoCanvas.zoom.closeMinimap", "关闭小地图") : canvasT("videoCanvas.zoom.openMinimap", "打开小地图"),
            icon: <Compass />,
            active: isMiniMapOpen,
            onClick: onToggleMiniMap,
        },
        { id: "zoom-fit", label: canvasT("videoCanvas.zoom.fitAll", "适应全部内容"), icon: <Focus />, onClick: onReset },
        ...(onAutoArrange ? [{ id: "zoom-auto-arrange", label: canvasT("videoCanvas.zoom.autoArrange", "整理画布"), icon: <LayoutTemplate />, onClick: onAutoArrange }] : []),
    ];
    const zoomItems: FloatingDockEntry[] = [
        { id: "zoom-out", label: canvasT("videoCanvas.zoom.zoomOut", "缩小画布"), icon: <Minus />, onClick: () => commitScale(liveScaleRef.current - 0.1) },
        {
            id: "zoom-precision",
            label: canvasT("videoCanvas.zoom.precision", "精确缩放"),
            wide: true,
            quiet: true,
            icon: (
                <span className="inline-flex h-full items-center justify-center whitespace-nowrap text-[var(--fs-caption)] font-semibold leading-none tabular-nums">
                    <span ref={dockLabelRef}>{Math.round(scale * 100)}</span>
                    <span className="ml-px text-[var(--fs-label)] font-medium leading-none opacity-50">%</span>
                </span>
            ),
            active: precisionOpen,
            onClick: () => setPrecisionOpen((value) => !value),
        },
        { id: "zoom-in", label: canvasT("videoCanvas.zoom.zoomIn", "放大画布"), icon: <Plus />, onClick: () => commitScale(liveScaleRef.current + 0.1) },
    ];
    const helpItems: FloatingDockEntry[] = [
        { id: "zoom-shortcuts", label: canvasT("videoCanvas.zoom.shortcuts", "画布快捷键"), icon: <HelpCircle />, onClick: onOpenShortcuts },
    ];

    return (
        <div ref={rootRef} data-canvas-no-zoom className="relative z-[var(--z-toolbar)] flex items-end gap-1.5" onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()} onWheel={(event) => event.stopPropagation()}>
            <FloatingDock items={navigateItems} size="compact" magnify={false} className="canvas-floating-dock" style={dockStyle} ariaLabel={canvasT("videoCanvas.zoom.dockAria", "画布视图控制")} />
            <div className="relative">
                {precisionOpen ? (
                    <CanvasOverlay theme={theme} className="absolute bottom-[var(--canvas-dock-popover-offset)] left-0 w-[200px] overflow-hidden p-2.5">
                        <div className="flex items-center justify-between gap-3">
                            <span className="text-[var(--fs-label)] font-medium">{canvasT("videoCanvas.zoom.scaleTitle", "画布尺度")}</span>
                            <span ref={panelLabelRef} className="text-[var(--fs-label)] font-semibold tabular-nums" style={{ color: theme.node.muted }}>
                                {Math.round(scale * 100)}%
                            </span>
                        </div>
                        <input
                            ref={rangeRef}
                            type="range"
                            min="5"
                            max="200"
                            step="1"
                            defaultValue={Math.round(scale * 100)}
                            className="aceternity-zoom-range mt-2 h-4 w-full"
                            style={{ accentColor: theme.node.text }}
                            onChange={(event) => commitScale(Number(event.target.value) / 100)}
                            aria-label={canvasT("videoCanvas.zoom.precisionAria", "精确缩放画布")}
                        />
                        <div className="mt-2 grid grid-cols-4 gap-0.5">
                            {QUICK_ZOOM_LEVELS.map((level) => (
                                <button
                                    key={level}
                                    type="button"
                                    className="h-7 rounded-md text-[var(--fs-label)] font-medium tabular-nums outline-none hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/8"
                                    style={{ color: theme.node.muted }}
                                    onClick={() => commitScale(level)}
                                >
                                    {Math.round(level * 100)}%
                                </button>
                            ))}
                        </div>
                    </CanvasOverlay>
                ) : null}
                <FloatingDock items={zoomItems} size="compact" magnify={false} className="canvas-floating-dock" style={dockStyle} ariaLabel={canvasT("videoCanvas.zoom.precision", "精确缩放")} />
            </div>
            <FloatingDock items={helpItems} size="compact" magnify={false} className="canvas-floating-dock" style={dockStyle} ariaLabel={canvasT("videoCanvas.zoom.shortcuts", "画布快捷键")} />
        </div>
    );
}
