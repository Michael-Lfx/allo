import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Segmented, Slider, Tooltip } from "antd";
import { motion, useReducedMotion } from "motion/react";
import { Camera, RotateCcw, Send, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { SpotlightSurface } from "@oc/components/ui/aceternity/spotlight-surface";
import { aceternityMotion } from "@oc/lib/aceternity-motion";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";

export type CanvasImageAngleParams = {
    horizontalAngle: number;
    pitchAngle: number;
    cameraDistance: number;
    wideAngle: boolean;
};

const defaultParams: CanvasImageAngleParams = { horizontalAngle: 0, pitchAngle: 0, cameraDistance: 4.8, wideAngle: false };

function anglePresets() {
    return [
        { id: "front", label: canvasT("videoCanvas.dialog.angleFront", "正面"), horizontalAngle: 0, pitchAngle: 0 },
        { id: "left", label: canvasT("videoCanvas.dialog.angleLeft", "左侧"), horizontalAngle: -90, pitchAngle: 0 },
        { id: "right", label: canvasT("videoCanvas.dialog.angleRight", "右侧"), horizontalAngle: 90, pitchAngle: 0 },
        { id: "back", label: canvasT("videoCanvas.dialog.angleBack", "背面"), horizontalAngle: 180, pitchAngle: 0 },
        { id: "top", label: canvasT("videoCanvas.dialog.angleTop", "俯拍"), horizontalAngle: 0, pitchAngle: 55 },
        { id: "bottom", label: canvasT("videoCanvas.dialog.angleBottom", "仰拍"), horizontalAngle: 0, pitchAngle: -45 },
    ];
}

export function CanvasNodeAnglePanel({ dataUrl, onClose, onConfirm }: { dataUrl: string; onClose: () => void; onConfirm: (params: CanvasImageAngleParams) => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const reducedMotion = useReducedMotion();
    const [params, setParams] = useState(defaultParams);
    const dragRef = useRef<{ x: number; y: number; horizontal: number; pitch: number } | null>(null);
    const presets = anglePresets();
    const update = <Key extends keyof CanvasImageAngleParams>(key: Key, value: CanvasImageAngleParams[Key]) => setParams((current) => ({ ...current, [key]: value }));

    const startCameraDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
        event.preventDefault();
        event.stopPropagation();
        event.currentTarget.setPointerCapture(event.pointerId);
        dragRef.current = { x: event.clientX, y: event.clientY, horizontal: params.horizontalAngle, pitch: params.pitchAngle };
    };
    const moveCamera = (event: ReactPointerEvent<HTMLDivElement>) => {
        const drag = dragRef.current;
        if (!drag) return;
        update("horizontalAngle", clamp(Math.round(drag.horizontal + (event.clientX - drag.x) * 0.8), -180, 180));
        update("pitchAngle", clamp(Math.round(drag.pitch - (event.clientY - drag.y) * 0.55), -75, 75));
    };
    const marker = cameraMarker(params.horizontalAngle, params.pitchAngle);
    const activePreset = presets.find((preset) => preset.horizontalAngle === params.horizontalAngle && preset.pitchAngle === params.pitchAngle);
    const customLabel = canvasT("videoCanvas.dialog.angleCustom", "自定义");

    return (
        <SpotlightSurface
            data-canvas-no-zoom
            spotlightColor={theme.toolbar.itemHover}
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: 8, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            transition={reducedMotion ? { duration: 0 } : aceternityMotion.spring.panel}
            className="aceternity-floating-panel w-[580px] max-w-full overflow-hidden rounded-[var(--r-2xl)] border backdrop-blur-2xl"
            style={{ background: theme.spatial.elevated, borderColor: theme.toolbar.border, color: theme.node.text, boxShadow: `0 28px 80px ${theme.spatial.shadow}` }}
            onMouseDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
        >
            <div className="flex h-11 items-center gap-2 border-b px-2.5" style={{ borderColor: theme.toolbar.border }}>
                <span className="grid size-7 shrink-0 place-items-center rounded-[var(--r-md)]" style={{ background: theme.toolbar.itemHover }}><Camera className="size-3.5" /></span>
                <span className="text-xs font-semibold">{canvasT("videoCanvas.dialog.angleTitle", "多角度编辑器")}</span>
                <span className="min-w-0 flex-1" />
                <Tooltip title={canvasT("videoCanvas.dialog.angleClose", "关闭")}><button type="button" aria-label={canvasT("videoCanvas.dialog.angleCloseAria", "关闭多角度编辑器")} className="grid size-7 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10" onClick={onClose}><X className="size-3.5" /></button></Tooltip>
            </div>
            <div className="flex h-10 items-center gap-1 overflow-x-auto border-b px-2.5 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden" style={{ borderColor: theme.toolbar.border }}>
                <AnglePresetButton active={!activePreset} label={customLabel} theme={theme} />
                {presets.map((preset) => <AnglePresetButton key={preset.id} active={activePreset?.id === preset.id} label={preset.label} theme={theme} onClick={() => setParams((current) => ({ ...current, horizontalAngle: preset.horizontalAngle, pitchAngle: preset.pitchAngle }))} />)}
            </div>
            <div className="grid h-[208px] grid-cols-[188px_minmax(0,1fr)] gap-2.5 p-2.5">
                <div className="relative grid size-[188px] cursor-grab place-items-center overflow-hidden rounded-[var(--r-lg)] border active:cursor-grabbing" style={{ borderColor: theme.toolbar.border, background: theme.spatial.surface }} onPointerDown={startCameraDrag} onPointerMove={moveCamera} onPointerUp={(event) => { dragRef.current = null; event.currentTarget.releasePointerCapture(event.pointerId); }} onPointerCancel={() => { dragRef.current = null; }}>
                    <GlobeGrid color={theme.node.muted} />
                    <img src={dataUrl} alt={canvasT("videoCanvas.dialog.angleRefAlt", "角度参考")} className="relative z-10 size-[72px] rounded-[var(--r-md)] object-cover shadow-lg" draggable={false} style={{ transform: previewTransform(params) }} />
                    <div className="pointer-events-none absolute z-20 grid size-7 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border shadow" style={{ left: `${marker.x}%`, top: `${marker.y}%`, background: theme.toolbar.panel, borderColor: theme.node.activeStroke, color: theme.node.activeStroke }}><Camera className="size-3.5" /></div>
                    <span className="pointer-events-none absolute bottom-2 text-[var(--fs-micro)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.angleDragHint", "拖动调整摄影机")}</span>
                </div>
                <div className="flex min-w-0 flex-col justify-center gap-2 rounded-[var(--r-lg)] border px-2.5 py-2" style={{ background: theme.toolbar.itemHover, borderColor: theme.toolbar.border }}>
                    <AngleSlider label={canvasT("videoCanvas.dialog.angleHorizontal", "水平环绕")} value={params.horizontalAngle} min={-180} max={180} suffix="°" onChange={(value) => update("horizontalAngle", value)} />
                    <AngleSlider label={canvasT("videoCanvas.dialog.anglePitch", "垂直俯仰")} value={params.pitchAngle} min={-75} max={75} suffix="°" onChange={(value) => update("pitchAngle", value)} />
                    <AngleSlider label={canvasT("videoCanvas.dialog.angleDistance", "景别缩放")} value={params.cameraDistance} min={1} max={10} step={0.1} suffix={distanceLabel(params.cameraDistance)} onChange={(value) => update("cameraDistance", value)} />
                    <div className="grid h-8 grid-cols-[62px_minmax(0,1fr)] items-center gap-2">
                        <span className="text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.angleLens", "镜头")}</span>
                        <Segmented block size="small" value={params.wideAngle ? "wide" : "standard"} options={[{ label: canvasT("videoCanvas.dialog.angleStandard", "标准"), value: "standard" }, { label: canvasT("videoCanvas.dialog.angleWide", "广角"), value: "wide" }]} onChange={(value) => update("wideAngle", value === "wide")} />
                    </div>
                </div>
            </div>
            <div className="flex h-11 items-center gap-2 border-t px-3" style={{ borderColor: theme.toolbar.border }}>
                <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.angleCurrent", "当前视角")}</span>
                <span className="text-xs font-semibold">{activePreset?.label || customLabel}</span>
                <span className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{params.horizontalAngle}° / {params.pitchAngle}° · {params.cameraDistance.toFixed(1)} {distanceLabel(params.cameraDistance)}</span>
                <span className="flex-1" />
                <button type="button" className="flex h-8 items-center gap-1.5 rounded-[var(--dock-item-radius)] px-2 text-[var(--fs-label)] font-medium transition hover:bg-black/5 dark:hover:bg-white/10" onClick={() => setParams(defaultParams)}><RotateCcw className="size-3.5" />{canvasT("videoCanvas.dialog.angleReset", "重置")}</button>
                <motion.button type="button" whileHover={reducedMotion ? undefined : { y: -1 }} whileTap={reducedMotion ? undefined : { scale: 0.97 }} className="flex h-8 items-center gap-1.5 rounded-[var(--dock-item-radius)] px-3 text-[var(--fs-label)] font-semibold" style={{ background: theme.node.activeStroke, color: theme.node.panel }} onClick={() => onConfirm(params)}><Send className="size-3.5" />{canvasT("videoCanvas.dialog.angleGenerate", "生成新角度")}</motion.button>
            </div>
        </SpotlightSurface>
    );
}

function AngleSlider({ label, value, min, max, step = 1, suffix, onChange }: { label: string; value: number; min: number; max: number; step?: number; suffix: string; onChange: (value: number) => void }) {
    return <div className="grid h-8 grid-cols-[62px_minmax(0,1fr)_60px] items-center gap-2"><span className="text-[var(--fs-tiny)] font-medium opacity-60">{label}</span><Slider min={min} max={max} step={step} value={value} onChange={onChange} /><span className="text-right text-[var(--fs-tiny)] font-semibold">{Number.isInteger(value) ? value : value.toFixed(1)}{suffix.startsWith("°") ? suffix : ""}{!suffix.startsWith("°") ? ` ${suffix}` : ""}</span></div>;
}

function AnglePresetButton({ active, label, theme, onClick }: { active: boolean; label: string; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onClick?: () => void }) {
    return <button type="button" aria-pressed={active} className="h-7 shrink-0 rounded-[var(--r-md)] border px-2.5 text-[var(--fs-tiny)] font-medium transition hover:bg-black/5 dark:hover:bg-white/10" style={{ background: active ? theme.toolbar.activeBg : theme.spatial.surface, borderColor: active ? theme.spatial.glowStrong : theme.toolbar.border }} onClick={onClick}>{label}</button>;
}

function GlobeGrid({ color }: { color: string }) {
    return <svg aria-hidden="true" className="pointer-events-none absolute inset-5 h-[calc(100%-40px)] w-[calc(100%-40px)] opacity-35" viewBox="0 0 200 200" fill="none" stroke={color} strokeWidth="1"><circle cx="100" cy="100" r="82" /><ellipse cx="100" cy="100" rx="42" ry="82" /><ellipse cx="100" cy="100" rx="68" ry="82" /><ellipse cx="100" cy="100" rx="82" ry="28" /><ellipse cx="100" cy="100" rx="82" ry="56" /><path d="M18 100h164M100 18v164" /></svg>;
}

function cameraMarker(horizontal: number, pitch: number) {
    const horizontalRad = horizontal * Math.PI / 180;
    const pitchRad = pitch * Math.PI / 180;
    return { x: 50 + Math.sin(horizontalRad) * Math.cos(pitchRad) * 40, y: 50 - Math.sin(pitchRad) * 40 };
}

function previewTransform(params: CanvasImageAngleParams) {
    const scale = clamp(1.08 - params.cameraDistance * 0.035 - (params.wideAngle ? 0.08 : 0), 0.72, 1.08);
    return `perspective(520px) rotateY(${params.horizontalAngle * -0.18}deg) rotateX(${params.pitchAngle * 0.16}deg) scale(${scale})`;
}

function distanceLabel(value: number) {
    if (value <= 3) return canvasT("videoCanvas.dialog.angleNear", "近景");
    if (value >= 7) return canvasT("videoCanvas.dialog.angleFar", "全景");
    return canvasT("videoCanvas.dialog.angleMid", "中景");
}

function clamp(value: number, min: number, max: number) {
    return Math.min(max, Math.max(min, value));
}
