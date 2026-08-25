import { Palette, RotateCcw } from "lucide-react";

import { useCanvasNodeActions } from "@oc/components/canvas/canvas-node-action-context";
import { useUpstreamNodes } from "@oc/components/canvas/canvas-node-graph-context";
import { colorGradeCssFilter, DEFAULT_COLOR_GRADE, isNeutralColorGrade, type CanvasColorGrade } from "@oc/lib/canvas/canvas-color-grade";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";

type ColorGradeNodeContentProps = {
    node: CanvasNodeData;
    theme: CanvasTheme;
};

const SLIDERS: Array<{ key: keyof CanvasColorGrade; labelKey: string; label: string; min: number; max: number }> = [
    { key: "brightness", labelKey: "videoCanvas.extension.gradeBrightness", label: "亮度", min: 0, max: 200 },
    { key: "contrast", labelKey: "videoCanvas.extension.gradeContrast", label: "对比", min: 0, max: 200 },
    { key: "saturate", labelKey: "videoCanvas.extension.gradeSaturate", label: "饱和", min: 0, max: 200 },
    { key: "hueRotate", labelKey: "videoCanvas.extension.gradeHue", label: "色相", min: -180, max: 180 },
];

/**
 * 调色节点：吃上游图片，本地 CSS filter 预览。
 * 不上传；桌面端仅预览加工。
 */
export function ColorGradeNodeContent({ node, theme }: ColorGradeNodeContentProps) {
    const { updateMetadata } = useCanvasNodeActions();
    const upstream = useUpstreamNodes(node.id);
    const inherited = upstream.find((item) => getNodeResourceKind(item) === "image" && item.metadata?.content);
    const url = inherited?.metadata?.content || "";
    const grade = node.metadata?.colorGrade || DEFAULT_COLOR_GRADE;

    if (!url) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center" style={{ color: theme.node.muted }}>
                <Palette className="size-5 opacity-60" />
                <span style={{ fontSize: "var(--fs-label)" }}>{canvasT("videoCanvas.extension.gradeEmpty", "连接一张图片即可调色")}</span>
            </div>
        );
    }

    const editable = Boolean(updateMetadata);
    const update = (key: keyof CanvasColorGrade, value: number) => updateMetadata?.(node.id, { colorGrade: { ...grade, [key]: value } });

    return (
        <div className="flex h-full w-full flex-col overflow-hidden" style={{ background: theme.node.fill }}>
            <div className="relative min-h-0 flex-1">
                <img src={url} alt={node.title || canvasT("videoCanvas.node.colorgrade", "调色")} className="h-full w-full object-contain" draggable={false} style={{ filter: colorGradeCssFilter(grade) }} />
                {isNeutralColorGrade(grade) ? (
                    <span className="pointer-events-none absolute left-2 top-2 rounded px-1.5 py-0.5 text-white" style={{ fontSize: "var(--fs-tiny)", background: "rgba(79,110,232,.85)" }}>
                        {canvasT("videoCanvas.extension.gradeNeutral", "未调色")}
                    </span>
                ) : null}
            </div>

            {editable ? (
                <div
                    className="shrink-0 border-t px-2 py-1.5"
                    data-canvas-no-zoom
                    style={{ borderColor: theme.node.stroke }}
                    onWheel={(event) => event.stopPropagation()}
                    onMouseDown={(event) => event.stopPropagation()}
                >
                    {SLIDERS.map((slider) => (
                        <label key={slider.key} className="flex items-center gap-2" style={{ fontSize: "var(--fs-tiny)", color: theme.node.muted }}>
                            <span className="w-6 shrink-0">{canvasT(slider.labelKey, slider.label)}</span>
                            <input
                                type="range"
                                className="min-w-0 flex-1 accent-[#4f6ee8]"
                                min={slider.min}
                                max={slider.max}
                                value={grade[slider.key]}
                                onChange={(event) => update(slider.key, Number(event.target.value))}
                            />
                            <span className="w-8 shrink-0 text-right tabular-nums">{grade[slider.key]}</span>
                        </label>
                    ))}
                    <button
                        type="button"
                        className="mt-1 inline-flex items-center gap-1 rounded px-1 outline-none transition-colors hover:bg-black/5 dark:hover:bg-white/10"
                        style={{ fontSize: "var(--fs-tiny)", color: theme.node.muted }}
                        onClick={() => updateMetadata?.(node.id, { colorGrade: DEFAULT_COLOR_GRADE })}
                    >
                        <RotateCcw className="size-3" />
                        {canvasT("videoCanvas.extension.gradeReset", "复位")}
                    </button>
                </div>
            ) : null}
        </div>
    );
}
