import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";

import { cn } from "@oc/lib/utils";

type CanvasComposerPillProps = ButtonHTMLAttributes<HTMLButtonElement> & {
    icon?: ReactNode;
    label: string;
    expanded?: boolean;
    ref?: Ref<HTMLButtonElement>;
};

export function CanvasComposerPill({
    ref,
    icon,
    label,
    expanded,
    className,
    ...props
}: CanvasComposerPillProps) {
    return (
        <button
            ref={ref}
            type="button"
            className={cn("canvas-composer-pill", className)}
            aria-expanded={expanded}
            {...props}
        >
            {icon}
            <span>{label}</span>
        </button>
    );
}
