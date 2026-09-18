import { useCallback, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { SlidersHorizontal } from "lucide-react";
import { useTranslation } from "react-i18next";

import { overlayPanelStyle, useAnchoredOverlay } from "@oc/components/canvas/canvas-overlay";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { anchoredOverlayStyle, type OverlayPlacement } from "@oc/lib/canvas/canvas-overlay";
import {
    PORTRAIT_TEXTURE_GROUPS,
    normalizePortraitTextureSettings,
    type PortraitTextureSettingKey,
    type PortraitTextureSettings,
} from "@oc/lib/canvas/canvas-portrait-texture";
import { useThemeStore } from "@oc/stores/use-theme-store";

type CanvasPortraitTexturePopoverProps = {
    value: unknown;
    placement?: OverlayPlacement;
    onChange: (settings: PortraitTextureSettings) => void;
};

export function CanvasPortraitTexturePopover({ value, placement = "topLeft", onChange }: CanvasPortraitTexturePopoverProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const settings = normalizePortraitTextureSettings(value);
    const buttonRef = useRef<HTMLButtonElement>(null);
    const panelRef = useRef<HTMLDivElement>(null);
    const [open, setOpen] = useState(false);
    const close = useCallback(() => setOpen(false), []);
    const rect = useAnchoredOverlay(open, buttonRef, panelRef, close);
    const geometry = rect ? anchoredOverlayStyle(rect, { width: window.innerWidth, height: window.innerHeight }, { width: 350, placement }) : null;

    const updateSetting = (key: PortraitTextureSettingKey, nextValue: string) => {
        onChange(normalizePortraitTextureSettings({ ...settings, [key]: nextValue }));
    };

    return (
        <>
            <button
                ref={buttonRef}
                type="button"
                className="canvas-chrome-token flex min-w-0 items-center gap-1"
                style={{ background: theme.accent.primarySoft, color: theme.accent.primary }}
                aria-expanded={open}
                aria-label={canvasT("videoCanvas.portrait.openAria", "打开人物质感调节面板")}
                onClick={() => setOpen((current) => !current)}
            >
                <SlidersHorizontal className="size-3 shrink-0" />
                <span className="truncate text-[var(--fs-tiny)] font-medium">{canvasT("videoCanvas.portrait.title", "人物质感调节")}</span>
            </button>
            {open && geometry
                ? createPortal(
                    <div
                        ref={panelRef}
                        className="canvas-overlay"
                        style={overlayPanelStyle(theme, geometry)}
                        onPointerDown={(event) => event.stopPropagation()}
                        onMouseDown={(event) => event.stopPropagation()}
                    >
                        <div className="w-full" style={{ color: theme.node.text }}>
                            <div className="mb-2 flex items-center gap-1.5 px-0.5">
                                <SlidersHorizontal className="size-3.5" style={{ color: theme.accent.primary }} />
                                <span className="text-xs font-medium">{canvasT("videoCanvas.portrait.title", "人物质感调节")}</span>
                            </div>
                            <div className="space-y-1">
                                {PORTRAIT_TEXTURE_GROUPS.map((group) => {
                                    const groupLabel = canvasT(`videoCanvas.portrait.groups.${group.key}`, group.label);
                                    return (
                                        <div key={group.key} className="grid grid-cols-[60px_minmax(0,1fr)] items-center gap-2 py-1">
                                            <span className="text-[var(--fs-label)]" style={{ color: theme.node.muted }}>{groupLabel}</span>
                                            <div className="grid min-w-0 grid-cols-3 gap-1" role="radiogroup" aria-label={groupLabel}>
                                                {group.options.map((option) => {
                                                    const selected = settings[group.key] === option.value;
                                                    return (
                                                        <button
                                                            key={option.value}
                                                            type="button"
                                                            role="radio"
                                                            aria-checked={selected}
                                                            className="h-7 min-w-0 rounded-md border px-1 text-[var(--fs-tiny)] transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-1 motion-reduce:transition-none"
                                                            style={{
                                                                background: selected ? theme.accent.primarySoft : theme.toolbar.itemHover,
                                                                borderColor: selected ? theme.accent.primary : "transparent",
                                                                color: selected ? theme.accent.primary : theme.node.muted,
                                                                outlineColor: theme.accent.primary,
                                                            }}
                                                            onClick={() => updateSetting(group.key, option.value)}
                                                        >
                                                            <span className="block truncate">{canvasT(`videoCanvas.portrait.options.${group.key}.${option.value}`, option.label)}</span>
                                                        </button>
                                                    );
                                                })}
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>
                        </div>
                    </div>,
                    document.body,
                )
                : null}
        </>
    );
}
