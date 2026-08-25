import { FileText } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { useUpstreamNodes } from "@oc/components/canvas/canvas-node-graph-context";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";

type MarkdownNodeContentProps = {
    node: CanvasNodeData;
    theme: CanvasTheme;
};

/**
 * Markdown 展示节点。
 * 内容取自身 metadata.content；为空时回落到上游文本素材。
 */
export function MarkdownNodeContent({ node, theme }: MarkdownNodeContentProps) {
    const upstream = useUpstreamNodes(node.id);
    const own = node.metadata?.content || "";
    const inherited = upstream.find((item) => getNodeResourceKind(item) === "text");
    const source = own || inherited?.metadata?.content || inherited?.metadata?.prompt || "";

    if (!source.trim()) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center" style={{ color: theme.node.muted }}>
                <FileText className="size-5 opacity-60" />
                <span style={{ fontSize: "var(--fs-label)" }}>{canvasT("videoCanvas.extension.markdownEmpty", "连接文本节点，或写入 Markdown")}</span>
            </div>
        );
    }

    return (
        <div
            className="canvas-markdown-node h-full w-full overflow-y-auto overflow-x-hidden px-4 py-3 [&_h1]:mb-2 [&_h1]:text-lg [&_h1]:font-semibold [&_h2]:mb-2 [&_h2]:text-base [&_h2]:font-semibold [&_h3]:mb-1 [&_h3]:text-sm [&_h3]:font-semibold [&_p]:mb-2 [&_ul]:mb-2 [&_ul]:list-disc [&_ul]:pl-4 [&_ol]:mb-2 [&_ol]:list-decimal [&_ol]:pl-4 [&_code]:rounded [&_code]:px-1 [&_pre]:mb-2 [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:p-2 [&_a]:underline"
            data-canvas-no-zoom
            style={{ color: theme.node.text }}
            onWheel={(event) => event.stopPropagation()}
            onMouseDown={(event) => event.stopPropagation()}
        >
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{source}</ReactMarkdown>
        </div>
    );
}
