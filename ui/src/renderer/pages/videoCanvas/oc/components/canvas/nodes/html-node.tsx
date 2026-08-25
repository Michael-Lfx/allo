import { Code } from "lucide-react";

import { useUpstreamNodes } from "@oc/components/canvas/canvas-node-graph-context";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeResourceKind } from "@oc/lib/canvas/node-registry";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";

import { SandboxedFrame } from "./sandboxed-frame";

type HtmlNodeContentProps = {
    node: CanvasNodeData;
    theme: CanvasTheme;
};

/**
 * HTML 展示节点：在沙箱 iframe 里预览 HTML。
 * 自身有源码时，上游文本作为数据填进 `{{input}}`。
 * 脚本可执行（allow-scripts），但不同时给 allow-same-origin。
 */
export function HtmlNodeContent({ node, theme }: HtmlNodeContentProps) {
    const upstream = useUpstreamNodes(node.id);
    const inherited = upstream.find((item) => getNodeResourceKind(item) === "text");
    const upstreamText = inherited?.metadata?.content || inherited?.metadata?.prompt || "";
    const own = node.metadata?.content || "";
    const source = own || upstreamText;
    const input = own ? upstreamText : "";

    if (!source.trim()) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center gap-2 px-4 text-center" style={{ color: theme.node.muted }}>
                <Code className="size-5 opacity-60" />
                <span style={{ fontSize: "var(--fs-label)" }}>{canvasT("videoCanvas.extension.htmlEmpty", "连接输出 HTML 的文本节点")}</span>
                <span style={{ fontSize: "var(--fs-tiny)" }}>{canvasT("videoCanvas.extension.htmlInputHint", "源码里的 {{input}} 会替换为上游文本")}</span>
            </div>
        );
    }

    return <SandboxedFrame srcDoc={source.replaceAll("{{input}}", input)} theme={theme} allowScripts />;
}
