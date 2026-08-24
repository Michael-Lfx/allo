import { useCallback } from "react";
import copyToClipboard from "copy-to-clipboard";
import { resourceFileUrl, resourceIdFromStorageKey } from "@oc/services/api/resources";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { App } from "antd";
import { CanvasNodeType, type CanvasNodeData, type Position } from "@oc/types/canvas";
import type { useCanvasNodeOperations } from "./use-canvas-node-operations";
import type { useCanvasUpload } from "./use-canvas-upload";

type CanvasClipboardActionsInput = {
    message: ReturnType<typeof App.useApp>["message"];
    shouldPreferCopiedNodes: ReturnType<typeof useCanvasNodeOperations>["shouldPreferCopiedNodes"];
    pasteCopiedNodes: ReturnType<typeof useCanvasNodeOperations>["pasteCopiedNodes"];
    pasteSystemClipboard: ReturnType<typeof useCanvasUpload>["pasteSystemClipboard"];
    releaseCopiedNodesPastePriority: ReturnType<typeof useCanvasNodeOperations>["releaseCopiedNodesPastePriority"];
};

export function useCanvasClipboardActions(input: CanvasClipboardActionsInput) {
    const { message, shouldPreferCopiedNodes, pasteCopiedNodes, pasteSystemClipboard, releaseCopiedNodesPastePriority } = input;

    const pasteAtPosition = useCallback(
        (position: Position) => {
            if (shouldPreferCopiedNodes() && pasteCopiedNodes(position)) return;
            void (async () => {
                try {
                    // 标记写入成功时仍优先系统图片，兼容截图和从外部应用复制的媒体。
                    const handled = await pasteSystemClipboard(position);
                    if (!handled) pasteCopiedNodes(position);
                } catch {
                    if (!pasteCopiedNodes(position)) message.warning(canvasT("videoCanvas.toast.clipboardUnreadable", "无法读取剪贴板内容"));
                }
            })();
        },
        [message, pasteCopiedNodes, pasteSystemClipboard, shouldPreferCopiedNodes],
    );

    const copyNodeContentToClipboard = useCallback(
        async (node: CanvasNodeData | null) => {
            releaseCopiedNodesPastePriority();
            const content = node?.metadata?.content;
            if (!node || !content) {
                message.warning(canvasT("videoCanvas.toast.nothingToCopy", "没有可复制的内容"));
                return;
            }

            try {
                if (node.type === CanvasNodeType.Image && typeof ClipboardItem !== "undefined" && navigator.clipboard?.write) {
                    const response = await fetch(content);
                    const blob = await response.blob();
                    await navigator.clipboard.write([new ClipboardItem({ [blob.type || "image/png"]: blob })]);
                    message.success(canvasT("videoCanvas.toast.imageCopied", "图片已复制"));
                    return;
                }

                if (!navigator.clipboard?.writeText) {
                    message.warning(canvasT("videoCanvas.toast.clipboardWriteUnsupported", "当前浏览器不支持写入剪贴板"));
                    return;
                }
                await navigator.clipboard.writeText(content);
                message.success(node.type === CanvasNodeType.Text ? canvasT("videoCanvas.toast.textCopied", "文本已复制") : canvasT("videoCanvas.toast.linkCopied", "内容链接已复制"));
            } catch {
                message.error(canvasT("videoCanvas.toast.copyFailed", "复制失败，请检查浏览器剪贴板权限"));
            }
        },
        [message, releaseCopiedNodesPastePriority],
    );

    const copyNodeMediaUrlToClipboard = useCallback(
        async (node: CanvasNodeData | null) => {
            releaseCopiedNodesPastePriority();
            try {
                const storageKey = node?.metadata?.storageKey;
                const content = node?.metadata?.content?.trim();
                const resourceId = resourceIdFromStorageKey(storageKey);
                const mediaPath = content && !content.startsWith("data:") && !content.startsWith("blob:") ? content : resourceId ? resourceFileUrl(resourceId) : "";
                const mediaURL = mediaPath ? new URL(mediaPath, window.location.href).toString() : "";
                if (!mediaURL) throw new Error(canvasT("videoCanvas.toast.mediaLocalOnly", "当前媒体只有本地内容，没有可复制的地址"));
                if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(mediaURL);
                else if (!(await copyToClipboard(mediaURL))) throw new Error(canvasT("videoCanvas.toast.clipboardWriteUnsupportedShort", "当前浏览器不支持写入剪贴板"));
                message.success(node?.type === CanvasNodeType.Video ? canvasT("videoCanvas.toast.videoUrlCopied", "视频地址已复制") : canvasT("videoCanvas.toast.imageUrlCopied", "图片地址已复制"));
            } catch (error) {
                message.error(error instanceof Error ? error.message : canvasT("videoCanvas.toast.mediaUrlCopyFailed", "媒体地址复制失败"));
            }
        },
        [message, releaseCopiedNodesPastePriority],
    );
    return { pasteAtPosition, copyNodeContentToClipboard, copyNodeMediaUrlToClipboard };
}
