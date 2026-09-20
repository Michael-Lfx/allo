import { useCallback, useRef, useState, type ReactNode, type WheelEvent } from "react";
import { createPortal } from "react-dom";
import { getOcPortalHost } from "@oc/lib/oc-scope";
import { ChevronDown, ChevronUp, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { CameraApertureArt, CameraBodyArt, CameraFocalArt, CameraLensArt } from "@oc/components/canvas/canvas-camera-art";
import { CanvasChromeButton, CanvasToggle, overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import {
    CAMERA_APERTURE_VALUES,
    CAMERA_BODIES,
    CAMERA_FOCAL_VALUES,
    CAMERA_LENSES,
    cameraBodyOption,
    cameraLensOption,
    cameraRigIsActive,
    compactCameraRigToken,
    stepCameraIndex,
    type CanvasCameraRig,
    type CanvasCameraRigPatch,
} from "@oc/lib/canvas/canvas-camera-rig";
import { anchoredOverlayStyle, type OverlayPlacement } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { cn } from "@oc/lib/utils";
import { useThemeStore } from "@oc/stores/use-theme-store";

type CanvasCameraPickerProps = {
    rig: CanvasCameraRig;
    onChange: (patch: CanvasCameraRigPatch) => void;
    placement?: OverlayPlacement;
    buttonClassName?: string;
};

export function CanvasCameraPicker({ rig, onChange, placement = "top", buttonClassName }: CanvasCameraPickerProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const buttonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [open, setOpen] = useState(false);
    const active = cameraRigIsActive(rig);
    const optics = compactCameraRigToken(rig);
    const close = useCallback(() => setOpen(false), []);
    const rect = useAnchoredOverlay(open, buttonRef, panelRef, close);
    const geometry = rect
        ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 560, placement, estimatedHeight: 292, gap: 10 })
        : null;
    const bodyIndex = Math.max(0, CAMERA_BODIES.findIndex((item) => item.id === rig.body));
    const lensIndex = Math.max(0, CAMERA_LENSES.findIndex((item) => item.id === rig.lens));
    const focalIndex = Math.max(0, CAMERA_FOCAL_VALUES.indexOf(rig.focal));
    const apertureIndex = Math.max(0, CAMERA_APERTURE_VALUES.indexOf(rig.aperture));

    return (
        <>
            <CanvasChromeButton
                ref={buttonRef}
                className={cn("is-icon canvas-camera-trigger flex-none", buttonClassName)}
                expanded={open}
                aria-pressed={active || undefined}
                aria-label={optics ? canvasT("videoCanvas.settings.cameraAria", "摄像机：{{summary}}", { summary: optics }) : canvasT("videoCanvas.settings.camera", "摄像机")}
                title={optics ? canvasT("videoCanvas.settings.cameraTooltip", "摄像机 · {{summary}}", { summary: optics }) : canvasT("videoCanvas.settings.camera", "摄像机")}
                onClick={() => setOpen((current) => !current)}
            >
                <CameraTriggerIcon active={active || open} />
            </CanvasChromeButton>
            {open && geometry
                ? createPortal(
                    <div
                        ref={panelRef}
                        data-canvas-no-zoom
                        className="canvas-overlay canvas-camera-panel"
                        style={{
                            ...overlayPanelStyle(theme, geometry),
                            overflow: "hidden",
                            padding: "12px 14px 14px",
                            borderRadius: 20,
                        }}
                        onPointerDown={(event) => event.stopPropagation()}
                        onMouseDown={(event) => event.stopPropagation()}
                        onWheel={(event) => event.stopPropagation()}
                    >
                        <div className="canvas-camera-panel-header">
                            <span>{canvasT("videoCanvas.settings.camera", "摄像机")}</span>
                            <button
                                type="button"
                                className="canvas-camera-close"
                                onClick={close}
                                aria-label={canvasT("videoCanvas.feedback.close", "关闭")}
                            >
                                <X className="size-4" />
                            </button>
                        </div>
                        <div className="canvas-camera-deck">
                            <PickerColumn
                                label={canvasT("videoCanvas.settings.cameraColBody", "相机")}
                                peek={CAMERA_BODIES[stepCameraIndex(bodyIndex, CAMERA_BODIES.length, -1)]?.name}
                                caption={cameraBodyOption(rig.body).name}
                                onStep={(delta) => onChange({ cameraBody: CAMERA_BODIES[stepCameraIndex(bodyIndex, CAMERA_BODIES.length, delta)].id })}
                            >
                                <CameraBodyArt option={cameraBodyOption(rig.body)} />
                            </PickerColumn>
                            <PickerColumn
                                label={canvasT("videoCanvas.settings.cameraColLens", "镜头")}
                                peek={CAMERA_LENSES[stepCameraIndex(lensIndex, CAMERA_LENSES.length, -1)]?.short}
                                caption={cameraLensOption(rig.lens).name}
                                onStep={(delta) => onChange({ cameraLens: CAMERA_LENSES[stepCameraIndex(lensIndex, CAMERA_LENSES.length, delta)].id })}
                            >
                                <CameraLensArt option={cameraLensOption(rig.lens)} />
                            </PickerColumn>
                            <PickerColumn
                                label={canvasT("videoCanvas.settings.cameraColFocal", "焦距")}
                                peek={CAMERA_FOCAL_VALUES[stepCameraIndex(focalIndex, CAMERA_FOCAL_VALUES.length, -1)]}
                                caption={canvasT("videoCanvas.settings.cameraFocalUnit", "mm")}
                                onStep={(delta) => onChange({ cameraFocal: CAMERA_FOCAL_VALUES[stepCameraIndex(focalIndex, CAMERA_FOCAL_VALUES.length, delta)] })}
                            >
                                <CameraFocalArt mm={rig.focal} />
                            </PickerColumn>
                            <PickerColumn
                                label={canvasT("videoCanvas.settings.cameraColAperture", "光圈")}
                                peek={`f/${CAMERA_APERTURE_VALUES[stepCameraIndex(apertureIndex, CAMERA_APERTURE_VALUES.length, -1)]}`}
                                caption={`f/${rig.aperture}`}
                                onStep={(delta) => onChange({ cameraAperture: CAMERA_APERTURE_VALUES[stepCameraIndex(apertureIndex, CAMERA_APERTURE_VALUES.length, delta)] })}
                            >
                                <CameraApertureArt value={rig.aperture} />
                            </PickerColumn>
                        </div>
                        <div className="canvas-camera-power">
                            <span>{canvasT("videoCanvas.settings.cameraPowerOff", "关闭")}</span>
                            <CanvasToggle
                                theme={theme}
                                checked={active}
                                ariaLabel={canvasT("videoCanvas.settings.camera", "摄像机")}
                                onChange={(checked) => onChange({
                                    cameraEnabled: checked ? "true" : "false",
                                    cameraBody: rig.body,
                                    cameraLens: rig.lens,
                                    cameraFocal: rig.focal,
                                    cameraAperture: rig.aperture,
                                })}
                            />
                        </div>
                    </div>,
                    getOcPortalHost(),
                )
                : null}
        </>
    );
}

function PickerColumn({
    label,
    peek,
    caption,
    onStep,
    children,
}: {
    label: string;
    peek?: string;
    caption: string;
    onStep: (delta: number) => void;
    children: ReactNode;
}) {
    const onWheel = (event: WheelEvent<HTMLDivElement>) => {
        if (!event.deltaY) return;
        event.preventDefault();
        event.stopPropagation();
        onStep(event.deltaY > 0 ? 1 : -1);
    };
    return (
        <div className="canvas-camera-col" onWheel={onWheel}>
            <button type="button" className="canvas-camera-chevron" onClick={() => onStep(-1)} aria-label={canvasT("videoCanvas.settings.cameraPrev", "上一项")}>
                <ChevronUp />
            </button>
            <span className="canvas-camera-peek">{peek}</span>
            <div className="canvas-camera-card">
                <span className="canvas-camera-card-label">{label}</span>
                <div className="canvas-camera-card-art">{children}</div>
            </div>
            <button type="button" className="canvas-camera-chevron" onClick={() => onStep(1)} aria-label={canvasT("videoCanvas.settings.cameraNext", "下一项")}>
                <ChevronDown />
            </button>
            <span className="canvas-camera-caption">{caption}</span>
        </div>
    );
}

function CameraTriggerIcon({ active }: { active: boolean }) {
    return (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <circle cx="8" cy="8" r="5.6" stroke="currentColor" strokeWidth="1.4" opacity={active ? 1 : 0.82} />
            <circle cx="8" cy="8" r="2.1" fill="currentColor" opacity={active ? 0.95 : 0.55} />
            <circle cx="8" cy="8" r="3.7" stroke="currentColor" strokeWidth="0.9" opacity="0.35" />
        </svg>
    );
}
