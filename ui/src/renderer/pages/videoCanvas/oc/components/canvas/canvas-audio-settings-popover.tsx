import { useCallback, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { CanvasChromeButton, overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { AudioSettingsPanel } from "@oc/components/audio-settings-panel";
import { audioVoiceLabel } from "@oc/lib/audio-generation";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { anchoredOverlayStyle, type OverlayPlacement } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { AiConfig } from "@oc/stores/use-config-store";

export type CanvasAudioSettingKey = "audioVoice" | "audioFormat" | "audioSpeed" | "audioInstructions";

type CanvasAudioSettingsPopoverProps = {
    config: AiConfig;
    onConfigChange: (key: CanvasAudioSettingKey, value: string) => void;
    buttonClassName?: string;
    placement?: OverlayPlacement;
};

export function CanvasAudioSettingsPopover({ config, onConfigChange, buttonClassName, placement = "topLeft" }: CanvasAudioSettingsPopoverProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const buttonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [open, setOpen] = useState(false);
    const summary = audioVoiceLabel(config.audioVoice);
    const close = useCallback(() => setOpen(false), []);
    const rect = useAnchoredOverlay(open, buttonRef, panelRef, close);
    const geometry = rect ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 356, placement }) : null;

    return (
        <>
            <CanvasChromeButton
                ref={buttonRef}
                className={buttonClassName}
                expanded={open}
                aria-label={canvasT("videoCanvas.settings.audioAria", "音频设置：{{summary}}", { summary })}
                title={canvasT("videoCanvas.settings.audioTooltip", "音频设置 · {{summary}}", { summary })}
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
                        <AudioSettingsPanel config={config} onConfigChange={onConfigChange} theme={theme} showTitle={false} className="space-y-4" />
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}
