import { LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { cn } from "@oc/lib/utils";
import { useThemeStore } from "@oc/stores/use-theme-store";

type CanvasComposerSendButtonProps = {
    disabled?: boolean;
    running?: boolean;
    sending?: boolean;
    sendLabel?: string;
    stopLabel?: string;
    onClick: () => void;
    className?: string;
};

export function CanvasComposerSendButton({
    disabled = false,
    running = false,
    sending = false,
    sendLabel,
    stopLabel,
    onClick,
    className,
}: CanvasComposerSendButtonProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const sendText = sendLabel ?? canvasT("videoCanvas.agent.send", "发送");
    const stopText = stopLabel ?? canvasT("videoCanvas.config.stop", "停止");
    const locked = !running && (disabled || sending);
    const muted = !running && !sending && disabled;
    const label = running ? stopText : sendText;

    return (
        <button
            type="button"
            className={cn("canvas-send-token", running && "is-stop", muted && "is-disabled", className)}
            disabled={locked}
            style={{
                background: muted ? theme.toolbar.itemHover : theme.node.activeStroke,
                color: muted ? theme.node.faint : theme.canvas.background,
            }}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={onClick}
            aria-label={label}
            title={label}
        >
            {sending && !running ? <LoaderCircle className="size-3.5 animate-spin" /> : running ? <span className="canvas-send-token-stop" /> : <SendArrow />}
        </button>
    );
}

function SendArrow() {
    return (
        <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M8 12.5V3.6M4.15 7.15 8 3.4l3.85 3.75" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
    );
}
