import { motion, useReducedMotion } from "motion/react";
import { Bot, PanelBottom, X, ZoomIn, ZoomOut } from "lucide-react";
import { Tooltip } from "antd";
import { useTranslation } from "react-i18next";

import { aceternityMotion } from "@oc/lib/aceternity-motion";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useThemeStore } from "@oc/stores/use-theme-store";

type CanvasFocusModeBarProps = {
    dockRevealed: boolean;
    agentOpen: boolean;
    zoomPercent: number;
    onToggleDock: () => void;
    onToggleAgent: () => void;
    onExit: () => void;
    onZoomIn: () => void;
    onZoomOut: () => void;
    onFit: () => void;
};

export function CanvasFocusModeBar({ dockRevealed, agentOpen, zoomPercent, onToggleDock, onToggleAgent, onExit, onZoomIn, onZoomOut, onFit }: CanvasFocusModeBarProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const reducedMotion = useReducedMotion();

    return (
        <motion.div
            initial={reducedMotion ? { opacity: 0 } : { opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={reducedMotion ? { duration: 0 } : aceternityMotion.spring.panel}
            className="pointer-events-auto absolute left-1/2 top-2 z-[var(--z-toolbar)] -translate-x-1/2"
            role="toolbar"
            aria-label={canvasT("videoCanvas.focus.toolbarAria", "专注模式工具栏")}
        >
            <div className="flex items-center gap-0.5 rounded-full p-1 backdrop-blur-2xl" style={{ background: theme.spatial.elevated, color: theme.node.text, boxShadow: "var(--workspace-overlay-shadow)" }}>
                <Tooltip title={canvasT("videoCanvas.focus.exitEsc", "退出专注模式（Esc）")}>
                    <button
                        type="button"
                        onClick={onExit}
                        className="grid size-8 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10"
                        style={{ color: theme.node.text }}
                        aria-label={canvasT("videoCanvas.focus.exit", "退出专注模式")}
                    >
                        <X className="size-4" />
                    </button>
                </Tooltip>
                <span className="mx-0.5 h-4 w-px" style={{ background: theme.toolbar.border }} />
                <Tooltip title={dockRevealed ? canvasT("videoCanvas.focus.collapseTools", "收起工具") : canvasT("videoCanvas.focus.tools", "工具")}>
                    <button
                        type="button"
                        onClick={onToggleDock}
                        className="grid size-8 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10"
                        style={{ color: theme.node.text, background: dockRevealed ? theme.toolbar.itemHover : undefined }}
                        aria-label={canvasT("videoCanvas.focus.tools", "工具")}
                        aria-pressed={dockRevealed}
                    >
                        <PanelBottom className="size-4" />
                    </button>
                </Tooltip>
                <Tooltip title={agentOpen ? canvasT("videoCanvas.focus.collapseAgent", "收起智能体") : canvasT("videoCanvas.focus.agent", "智能体")}>
                    <button
                        type="button"
                        onClick={onToggleAgent}
                        className="grid size-8 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10"
                        style={{ color: theme.node.text, background: agentOpen ? theme.toolbar.itemHover : undefined }}
                        aria-label={canvasT("videoCanvas.focus.agent", "智能体")}
                        aria-pressed={agentOpen}
                    >
                        <Bot className="size-4" />
                    </button>
                </Tooltip>
                <span className="mx-0.5 h-4 w-px" style={{ background: theme.toolbar.border }} />
                <Tooltip title={canvasT("videoCanvas.focus.zoomOut", "缩小")}>
                    <button type="button" onClick={onZoomOut} className="grid size-8 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10" style={{ color: theme.node.text }} aria-label={canvasT("videoCanvas.focus.zoomOut", "缩小")}>
                        <ZoomOut className="size-4" />
                    </button>
                </Tooltip>
                <button
                    type="button"
                    onClick={onFit}
                    className="grid h-8 min-w-14 place-items-center rounded-full px-2 text-xs font-medium tabular-nums transition hover:bg-black/5 dark:hover:bg-white/10"
                    style={{ color: theme.node.text }}
                    title={canvasT("videoCanvas.focus.fitCanvas", "适应画布")}
                >
                    {Math.round(zoomPercent * 100)}%
                </button>
                <Tooltip title={canvasT("videoCanvas.focus.zoomIn", "放大")}>
                    <button type="button" onClick={onZoomIn} className="grid size-8 place-items-center rounded-full transition hover:bg-black/5 dark:hover:bg-white/10" style={{ color: theme.node.text }} aria-label={canvasT("videoCanvas.focus.zoomIn", "放大")}>
                        <ZoomIn className="size-4" />
                    </button>
                </Tooltip>
            </div>
        </motion.div>
    );
}
