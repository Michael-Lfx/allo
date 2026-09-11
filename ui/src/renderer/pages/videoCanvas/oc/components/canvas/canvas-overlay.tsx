import { useEffect, useRef, useState, type ButtonHTMLAttributes, type CSSProperties, type HTMLAttributes, type ReactNode, type Ref, type RefObject } from "react";
import { createPortal } from "react-dom";
import { ChevronRight, X } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { canvasThemes, type CanvasTheme } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";
import { cn } from "@oc/lib/utils";

export function CanvasOverlay({
    ref,
    theme,
    className,
    style,
    children,
    ...props
}: HTMLAttributes<HTMLDivElement> & { theme: CanvasTheme; ref?: Ref<HTMLDivElement> }) {
    return (
        <div ref={ref} className={cn("canvas-overlay", className)} style={{ ...canvasOverlayStyle(theme), ...style }} {...props}>
            {children}
        </div>
    );
}

export function CanvasMenuRow({
    icon,
    label,
    detail,
    shortcut,
    badge,
    chevron = false,
    active = false,
    disabled = false,
    danger = false,
    onClick,
}: {
    icon?: ReactNode;
    label: string;
    detail?: string;
    shortcut?: string;
    badge?: string;
    chevron?: boolean;
    active?: boolean;
    disabled?: boolean;
    danger?: boolean;
    onClick?: () => void;
}) {
    return (
        <button
            type="button"
            className="canvas-menu-row"
            disabled={disabled}
            aria-pressed={active || undefined}
            onClick={onClick}
            data-active={active ? "" : undefined}
            data-danger={danger ? "" : undefined}
        >
            {icon ? <span className="canvas-menu-row-icon">{icon}</span> : null}
            <span className="min-w-0 flex-1 text-left">
                <span className="flex items-center gap-1">
                    <span className="truncate">{label}</span>
                    {badge ? <span className="canvas-menu-row-badge">{badge}</span> : null}
                </span>
                {detail ? <span className="canvas-menu-row-detail">{detail}</span> : null}
            </span>
            {shortcut ? <span className="canvas-menu-row-shortcut">{shortcut}</span> : null}
            {chevron ? <ChevronRight className="size-3 shrink-0 opacity-40" /> : null}
        </button>
    );
}

export function CanvasHoverHint({
    label,
    disabled = false,
    children,
}: {
    label?: string;
    disabled?: boolean;
    children: ReactNode;
}) {
    const wrapRef = useRef<HTMLSpanElement>(null);
    const [open, setOpen] = useState(false);
    const [pos, setPos] = useState({ left: 0, barTop: 0, barBottom: 0, below: false });
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const hint = label?.trim() || "";

    const sync = () => {
        const wrap = wrapRef.current;
        if (!wrap) return;
        const button = wrap.getBoundingClientRect();
        const bar =
            wrap.closest(".aceternity-floating-dock")?.getBoundingClientRect() ??
            wrap.closest(".canvas-node-toolbar")?.getBoundingClientRect() ??
            button;
        const below = bar.top < 36;
        setPos({
            left: Math.min(Math.max(button.left + button.width / 2, 72), window.innerWidth - 72),
            barTop: bar.top,
            barBottom: bar.bottom,
            below,
        });
    };

    const show = () => {
        if (disabled || !hint) return;
        sync();
        setOpen(true);
    };

    return (
        <span
            ref={wrapRef}
            className="inline-flex shrink-0"
            onMouseEnter={show}
            onMouseMove={open ? sync : undefined}
            onMouseLeave={() => setOpen(false)}
            onFocusCapture={(event) => {
                if ((event.target as HTMLElement).matches(":focus-visible")) show();
            }}
            onBlurCapture={() => setOpen(false)}
        >
            {children}
            {open && hint
                ? createPortal(
                    <span
                        role="tooltip"
                        data-canvas-hover-hint=""
                        className="aceternity-dock-tooltip pointer-events-none fixed z-[var(--z-popover)] whitespace-nowrap rounded-md border px-2 py-0.5 text-[var(--fs-tiny)] font-medium shadow-md backdrop-blur-xl"
                        style={{
                            left: pos.left,
                            transform: "translateX(-50%)",
                            ...(pos.below
                                ? { top: pos.barBottom + 4 }
                                : { bottom: Math.max(0, window.innerHeight - pos.barTop + 4) }),
                            background: theme.spatial.elevated,
                            borderColor: theme.toolbar.border,
                            color: theme.toolbar.item,
                        }}
                    >
                        {hint}
                    </span>,
                    document.body,
                )
                : null}
        </span>
    );
}

export function CanvasMenuSeparator() {
    return <div className="canvas-menu-separator" role="separator" />;
}

export function CanvasChromeButton({
    ref,
    children,
    className,
    style,
    expanded,
    ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { expanded?: boolean; ref?: Ref<HTMLButtonElement> }) {
    return (
        <button
            ref={ref}
            type="button"
            className={cn("canvas-chrome-token inline-flex", className)}
            aria-expanded={expanded}
            style={style}
            {...props}
        >
            {children}
        </button>
    );
}

export function useAnchoredOverlay(
    open: boolean,
    triggerRef: RefObject<HTMLElement | null>,
    panelRef: RefObject<HTMLElement | null>,
    onClose: () => void,
): DOMRect | null {
    const [rect, setRect] = useState<DOMRect | null>(null);
    const onCloseRef = useRef(onClose);
    onCloseRef.current = onClose;

    useEffect(() => {
        if (!open) {
            setRect(null);
            return;
        }
        const sync = () => setRect(triggerRef.current?.getBoundingClientRect() ?? null);
        const closeOnOutside = (event: PointerEvent) => {
            const target = event.target;
            if (!(target instanceof Node)) return;
            if (triggerRef.current?.contains(target) || panelRef.current?.contains(target)) return;
            onCloseRef.current();
        };
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key === "Escape") onCloseRef.current();
        };
        sync();
        window.addEventListener("resize", sync);
        window.addEventListener("scroll", sync, true);
        window.addEventListener("pointerdown", closeOnOutside, true);
        window.addEventListener("keydown", closeOnEscape);
        return () => {
            window.removeEventListener("resize", sync);
            window.removeEventListener("scroll", sync, true);
            window.removeEventListener("pointerdown", closeOnOutside, true);
            window.removeEventListener("keydown", closeOnEscape);
        };
    }, [open, panelRef, triggerRef]);

    return rect;
}

export function overlayPanelStyle(theme: CanvasTheme, geometry: CSSProperties): CSSProperties {
    return {
        ...canvasOverlayStyle(theme),
        ...geometry,
        overflowY: "auto",
        padding: 12,
        borderRadius: 12,
        border: `1px solid ${theme.toolbar.border}`,
    };
}

export function CanvasSheet({
    open,
    title,
    subtitle,
    theme,
    width = 760,
    onClose,
    footer,
    children,
    className,
}: {
    open: boolean;
    title: ReactNode;
    subtitle?: ReactNode;
    theme: CanvasTheme;
    width?: number | string;
    onClose: () => void;
    footer?: ReactNode;
    children: ReactNode;
    className?: string;
}) {
    useEffect(() => {
        if (!open) return;
        const onKey = (event: KeyboardEvent) => {
            if (event.key === "Escape") onClose();
        };
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    }, [open, onClose]);

    if (!open) return null;

    return createPortal(
        <div className="canvas-sheet-root" role="presentation">
            <button type="button" className="canvas-sheet-mask" aria-label={canvasT("videoCanvas.sheet.close", "关闭")} onClick={onClose} />
            <div
                role="dialog"
                aria-modal="true"
                className={cn("canvas-overlay canvas-sheet", className)}
                style={{ ...canvasOverlayStyle(theme), width }}
                onMouseDown={(event) => event.stopPropagation()}
                onPointerDown={(event) => event.stopPropagation()}
            >
                <header className="canvas-sheet-header" style={{ borderColor: theme.toolbar.border }}>
                    <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-semibold">{title}</div>
                        {subtitle ? (
                            <div className="mt-0.5 truncate text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>
                                {subtitle}
                            </div>
                        ) : null}
                    </div>
                    <CanvasChromeButton className="is-icon shrink-0 self-start" aria-label={canvasT("videoCanvas.sheet.close", "关闭")} onClick={onClose}>
                        <X className="block size-3.5" strokeWidth={2} />
                    </CanvasChromeButton>
                </header>
                <div className="canvas-sheet-body thin-scrollbar">{children}</div>
                {footer ? (
                    <footer className="canvas-sheet-footer" style={{ borderColor: theme.toolbar.border }}>
                        {footer}
                    </footer>
                ) : null}
            </div>
        </div>,
        document.body,
    );
}

export function CanvasSheetButton({
    theme,
    variant = "ghost",
    disabled,
    onClick,
    children,
    className,
    "aria-label": ariaLabel,
}: {
    theme: CanvasTheme;
    variant?: "ghost" | "primary" | "danger";
    disabled?: boolean;
    onClick?: () => void;
    children: ReactNode;
    className?: string;
    "aria-label"?: string;
}) {
    const style =
        variant === "primary"
            ? { background: theme.node.activeStroke, color: theme.node.panel }
            : variant === "danger"
              ? { background: `${theme.accent.danger}22`, color: theme.accent.danger }
              : { background: theme.toolbar.itemHover, color: theme.node.text };
    return (
        <button type="button" disabled={disabled} aria-label={ariaLabel} className={cn("canvas-sheet-btn inline-flex items-center justify-center gap-1.5", className)} style={style} onClick={onClick}>
            {children}
        </button>
    );
}

export function CanvasToggle({
    checked,
    onChange,
    theme,
    ariaLabel,
}: {
    checked: boolean;
    onChange: (checked: boolean) => void;
    theme: CanvasTheme;
    ariaLabel?: string;
}) {
    return (
        <button
            type="button"
            role="switch"
            aria-checked={checked}
            aria-label={ariaLabel}
            className="canvas-toggle"
            style={{ background: checked ? theme.accent.primary : theme.toolbar.itemHover }}
            onClick={() => onChange(!checked)}
        >
            <span className="canvas-toggle-knob" data-on={checked ? "" : undefined} />
        </button>
    );
}

export function CanvasRange({
    value,
    min,
    max,
    step = 1,
    onChange,
    theme,
    ariaLabel,
}: {
    value: number;
    min: number;
    max: number;
    step?: number;
    onChange: (value: number) => void;
    theme: CanvasTheme;
    ariaLabel?: string;
}) {
    return (
        <input
            type="range"
            className="canvas-range"
            min={min}
            max={max}
            step={step}
            value={value}
            aria-label={ariaLabel}
            style={{ accentColor: theme.accent.primary }}
            onChange={(event) => onChange(Number(event.target.value))}
        />
    );
}
