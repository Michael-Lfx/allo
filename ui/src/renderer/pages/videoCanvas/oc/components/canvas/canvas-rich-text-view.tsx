import { useMemo } from "react";
import type { CSSProperties, WheelEventHandler } from "react";

import { canvasRichTextHTML } from "@oc/lib/canvas/canvas-rich-text";

type CanvasRichTextViewProps = {
    richText?: Record<string, unknown>;
    className: string;
    style: CSSProperties;
    onWheel?: WheelEventHandler<HTMLDivElement>;
};

/**
 * 只读富文本渲染，独立成懒加载组件：
 * tiptap 及其扩展只进本组件所在 chunk，不再被 canvas-node 拖进项目入口
 * （入口体积由 projectEntryChunk.test.ts 守护）。
 */
export default function CanvasRichTextView({ richText, className, style, onWheel }: CanvasRichTextViewProps) {
    const html = useMemo(() => canvasRichTextHTML(richText), [richText]);
    return <div className={className} style={style} onWheel={onWheel} dangerouslySetInnerHTML={{ __html: html }} />;
}
