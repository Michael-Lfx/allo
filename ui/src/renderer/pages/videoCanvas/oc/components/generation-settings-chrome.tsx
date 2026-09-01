import type { CSSProperties, ReactNode } from "react";

import type { CanvasTheme } from "@oc/lib/canvas-theme";

export function SettingsPanelHeader({ title, subtitle, theme }: { title: string; subtitle?: string; theme: CanvasTheme }) {
    return (
        <div className="mb-1">
            <div className="text-sm font-semibold tracking-tight">{title}</div>
            {subtitle ? <p className="mt-1 text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>{subtitle}</p> : null}
        </div>
    );
}

export function SettingsSection({ title, hint, extra, theme, children }: { title: string; hint?: string; extra?: ReactNode; theme: CanvasTheme; children?: ReactNode }) {
    return (
        <section className="rounded-xl border px-3 py-2.5" style={{ borderColor: theme.node.stroke, background: theme.node.fill }}>
            <div className={children ? "mb-2 flex items-center justify-between gap-3" : "flex items-center justify-between gap-3"}>
                <div className="min-w-0">
                    <div className="text-[var(--fs-tiny)] font-semibold tracking-wide" style={{ color: theme.node.muted }}>{title}</div>
                    {hint ? <div className="mt-0.5 text-[11px] leading-4" style={{ color: theme.node.muted }}>{hint}</div> : null}
                </div>
                {extra}
            </div>
            {children}
        </section>
    );
}

export function ChoiceChip({ selected, theme, disabled, onClick, children }: { selected: boolean; theme: CanvasTheme; disabled?: boolean; onClick: () => void; children: ReactNode }) {
    return (
        <button
            type="button"
            disabled={disabled}
            className="h-8 cursor-pointer rounded-full border px-2.5 text-[var(--fs-label)] font-medium leading-none transition-colors hover:brightness-110 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 disabled:cursor-not-allowed disabled:opacity-35"
            style={{
                background: selected ? theme.toolbar.activeBg : "transparent",
                borderColor: selected ? theme.node.activeStroke : theme.node.stroke,
                color: theme.node.text,
                boxShadow: selected ? `inset 0 0 0 1px ${theme.node.activeStroke}` : undefined,
                outlineColor: theme.node.muted,
            }}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={onClick}
        >
            {children}
        </button>
    );
}

export function AspectChoice({ selected, label, preview, theme, onClick }: { selected: boolean; label: string; preview: ReactNode; theme: CanvasTheme; onClick: () => void }) {
    return (
        <button
            type="button"
            className="flex h-[58px] cursor-pointer flex-col items-center justify-center gap-1 rounded-xl border text-[var(--fs-tiny)] font-medium transition-colors hover:brightness-110 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1"
            style={{
                background: selected ? theme.toolbar.activeBg : "transparent",
                borderColor: selected ? theme.node.activeStroke : theme.node.stroke,
                color: theme.node.text,
                boxShadow: selected ? `inset 0 0 0 1px ${theme.node.activeStroke}` : undefined,
                outlineColor: theme.node.muted,
            } as CSSProperties}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={onClick}
        >
            {preview}
            <span className="whitespace-nowrap">{label}</span>
        </button>
    );
}
