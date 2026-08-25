import { Shapes } from "lucide-react";

import { useUpstreamNodes } from "@oc/components/canvas/canvas-node-graph-context";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";

import { centeredFrameDocument, SandboxedFrame } from "./sandboxed-frame";

type SvgNodeContentProps = {
    node: CanvasNodeData;
    theme: CanvasTheme;
};

/** 只取到第一个 <svg> 根元素，避免上游文本里的说明文字被一起塞进文档。 */
function extractSvgSource(text: string) {
    const start = text.indexOf("<svg");
    if (start < 0) return "";
    const end = text.lastIndexOf("</svg>");
    return end > start ? text.slice(start, end + "</svg>".length) : "";
}

/**
 * SVG 展示节点：把 SVG 源码渲染成图形。
 * 渲染走沙箱 iframe，不给脚本权限。
 */
export function SvgNodeContent({ node, theme }: SvgNodeContentProps) {
    const upstream = useUpstreamNodes(node.id);
    const inherited = upstream.find((item) => getNodeResourceKind(item) === "text");
    const raw = node.metadata?.content || inherited?.metadata?.content || inherited?.metadata?.prompt || "";
    const svg = extractSvgSource(raw);

    if (!svg) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center" style={{ color: theme.node.muted }}>
                <Shapes className="size-5 opacity-60" />
                <span style={{ fontSize: "var(--fs-label)" }}>
                    {raw.trim()
                        ? canvasT("videoCanvas.extension.svgMissing", "上游内容里没有找到 <svg> 源码")
                        : canvasT("videoCanvas.extension.svgEmpty", "连接输出 SVG 的文本节点")}
                </span>
            </div>
        );
    }

    return <SandboxedFrame srcDoc={centeredFrameDocument(svg)} theme={theme} />;
}
