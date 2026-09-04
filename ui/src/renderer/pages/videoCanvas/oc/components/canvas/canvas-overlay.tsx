import { useEffect, useRef, useState, type CSSProperties, type HTMLAttributes, type ReactNode, type Ref, type RefObject } from "react";
import { ChevronRight } from "lucide-react";

import { canvasOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
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
}: HTMLAttributes<HTMLButtonElement> & { expanded?: boolean; ref?: Ref<HTMLButtonElement> }) {
    return (
        <button
            ref={ref}
            type="button"
            className={cn("canvas-chrome-token", className)}
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
