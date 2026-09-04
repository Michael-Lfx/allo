import { useCallback, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { CanvasChromeButton, overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { VideoSettingsPanel, videoResolutionLabel, videoSecondsLabel, videoSizeLabel } from "@oc/components/video-settings-panel";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { anchoredOverlayStyle, type OverlayPlacement } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { modelCapabilityConfigFor, resolveVideoRatioValue, resolveVideoResolutionValue } from "@oc/lib/model-capabilities";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { AiConfig } from "@oc/stores/use-config-store";

export type CanvasVideoSettingKey = "vquality" | "size" | "videoSeconds" | "videoGenerateAudio" | "videoWatermark";

type CanvasVideoSettingsPopoverProps = {
    config: AiConfig;
    onConfigChange: (key: CanvasVideoSettingKey, value: string) => void;
    buttonClassName?: string;
    placement?: OverlayPlacement;
};

export function CanvasVideoSettingsPopover({ config, onConfigChange, buttonClassName, placement = "topLeft" }: CanvasVideoSettingsPopoverProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const buttonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [open, setOpen] = useState(false);
    const videoProfile = modelCapabilityConfigFor(config, config.model).video;
    const resolutionSupported = Boolean(videoProfile?.resolutions.length);
    const sizeSupported = Boolean(videoProfile?.ratios.length);
    const resolution = videoProfile ? resolveVideoResolutionValue(videoProfile, config.vquality) : "";
    const size = videoProfile ? resolveVideoRatioValue(videoProfile, config.size) : "";
    const summary = [
        ...(sizeSupported ? [size.includes(":") ? size : videoSizeLabel(size)] : resolutionSupported ? [videoResolutionLabel(resolution)] : []),
        videoSecondsLabel(config.videoSeconds),
    ].join(" · ");
    const close = useCallback(() => setOpen(false), []);
    const rect = useAnchoredOverlay(open, buttonRef, panelRef, close);
    const geometry = rect ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 380, placement, estimatedHeight: 420 }) : null;

    return (
        <>
            <CanvasChromeButton
                ref={buttonRef}
                className={buttonClassName}
                expanded={open}
                aria-label={canvasT("videoCanvas.settings.videoAria", "视频设置：{{summary}}", { summary })}
                title={canvasT("videoCanvas.settings.videoTooltip", "视频设置 · {{summary}}", { summary })}
                onClick={() => setOpen((current) => !current)}
            >
                <span className="truncate">{summary}</span>
            </CanvasChromeButton>
            {open && geometry
                ? createPortal(
                    <div
                        ref={panelRef}
                        className="canvas-overlay"
                        style={overlayPanelStyle(theme, geometry)}
                        onPointerDown={(event) => event.stopPropagation()}
                        onMouseDown={(event) => event.stopPropagation()}
                    >
                        <VideoSettingsPanel config={config} onConfigChange={onConfigChange} theme={theme} showTitle={false} className="space-y-2.5" />
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}
