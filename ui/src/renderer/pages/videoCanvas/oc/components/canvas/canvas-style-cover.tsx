import type { CSSProperties, ReactNode } from "react";

import type { CanvasStyleCover } from "@oc/lib/canvas/canvas-style-system";

const FALLBACK_COVER: CanvasStyleCover = { from: "#5a6570", via: "#8a8070", to: "#d8d0c4" };

export function CanvasStyleCoverSwatch({
    cover,
    className,
    style,
    alt,
    children,
    hoverZoom = false,
}: {
    cover?: CanvasStyleCover | null;
    className?: string;
    style?: CSSProperties;
    alt?: string;
    children?: ReactNode;
    hoverZoom?: boolean;
}) {
    const stops = cover ?? FALLBACK_COVER;
    return (
        <span
            role={alt ? "img" : undefined}
            aria-label={alt}
            className={`relative block overflow-hidden ${className ?? ""}`}
            style={{
                background: `linear-gradient(128deg, ${stops.from} 0%, ${stops.via} 46%, ${stops.to} 100%)`,
                ...style,
            }}
        >
            <span
                className={`pointer-events-none absolute inset-0 opacity-[0.22] ${hoverZoom ? "transition duration-500 group-hover:scale-[1.06]" : ""}`}
                style={{
                    backgroundImage: "radial-gradient(ellipse at 18% 20%, rgba(255,255,255,.28), transparent 46%), repeating-linear-gradient(-18deg, rgba(255,255,255,.1) 0 1px, transparent 1px 8px)",
                }}
            />
            {children}
        </span>
    );
}
