import { useCallback, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getOcPortalHost } from "@oc/lib/oc-scope";
import { useTranslation } from "react-i18next";

import { CanvasChromeButton, overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { ImageSettingsPanel, imageQualityLabel, imageSizeLabel } from "@oc/components/image-settings-panel";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { CANVAS_IMAGE_BATCH_MAX_COUNT, getCanvasBatchCount } from "@oc/lib/canvas/canvas-generation-count";
import { imageCapabilityConfigFor } from "@oc/lib/model-capabilities";
import { anchoredOverlayStyle, type OverlayPlacement } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { AiConfig } from "@oc/stores/use-config-store";

type ImageSettingKey = "quality" | "size" | "transparentBackground" | "count";

type CanvasImageSettingsPopoverProps = {
    config: AiConfig;
    onConfigChange: (key: ImageSettingKey, value: string) => void;
    onOpenChange?: (open: boolean) => void;
    buttonClassName?: string;
    placement?: OverlayPlacement;
    showCount?: boolean;
};

export function CanvasImageSettingsPopover({ config, onConfigChange, onOpenChange, buttonClassName, placement = "topLeft", showCount = true }: CanvasImageSettingsPopoverProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const buttonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [open, setOpen] = useState(false);
    const quality = config.quality || "auto";
    const count = getCanvasBatchCount(config.count, CANVAS_IMAGE_BATCH_MAX_COUNT);
    const activeSize = config.size || "auto";
    const summary = compactImageToken(config, quality, activeSize, showCount ? count : 1);
    const close = useCallback(() => {
        setOpen(false);
        onOpenChange?.(false);
    }, [onOpenChange]);
    const rect = useAnchoredOverlay(open, buttonRef, panelRef, close);
    const geometry = rect ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 448, placement, estimatedHeight: 480 }) : null;

    return (
        <>
            <CanvasChromeButton
                ref={buttonRef}
                className={buttonClassName}
                expanded={open}
                aria-label={canvasT("videoCanvas.settings.imageAria", "图像设置：{{summary}}", { summary })}
                title={canvasT("videoCanvas.settings.imageTooltip", "图像设置 · {{summary}}", { summary })}
                onClick={() => {
                    const next = !open;
                    setOpen(next);
                    onOpenChange?.(next);
                }}
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
                        <ImageSettingsPanel config={config} onConfigChange={onConfigChange} theme={theme} showTitle={false} showCount={showCount} className="thin-scrollbar max-h-[min(72vh,640px)] space-y-2.5 overflow-y-auto pr-0.5" />
                    </div>,
                    getOcPortalHost(),
                )
                : null}
        </>
    );
}

function compactImageToken(config: AiConfig, quality: string, size: string, count: number) {
    const profile = imageCapabilityConfigFor(config, config.model);
    const parts = [imageSizeLabel(size, profile)];
    if (profile.qualities.length) parts.push(imageQualityLabel(quality));
    if (count > 1) parts.push(`×${count}`);
    return parts.join(" · ");
}
