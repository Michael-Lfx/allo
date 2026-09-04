import { useCallback, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { CanvasChromeButton, overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { ImageSettingsPanel, imageQualityLabel, imageSizeLabel } from "@oc/components/image-settings-panel";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
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
    const count = Math.max(1, Math.min(15, Math.floor(Math.abs(Number(config.count) || 1))));
    const activeSize = config.size || "auto";
    const summary = compactImageToken(quality, activeSize, showCount ? count : 1);
    const close = useCallback(() => {
        setOpen(false);
        onOpenChange?.(false);
    }, [onOpenChange]);
    const rect = useAnchoredOverlay(open, buttonRef, panelRef, close);
    const geometry = rect ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 440, placement }) : null;

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
                        <ImageSettingsPanel config={config} onConfigChange={onConfigChange} theme={theme} showTitle={false} showCount={showCount} className="space-y-2.5" />
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}

function compactImageToken(quality: string, size: string, count: number) {
    const parts = [imageSizeLabel(size), imageQualityLabel(quality)];
    if (count > 1) parts.push(`×${count}`);
    return parts.join(" · ");
}
