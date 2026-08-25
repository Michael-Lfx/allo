import { createContext, useContext } from "react";

import type { CanvasNodeData, CanvasNodeMetadata } from "@oc/types/canvas";

// 扩展节点（如调色）需要写 metadata，但节点经 WorldLayers 渲染不便透传 handler。
// 通过 Context 注入；无 Provider 时静默降级为 no-op。
export type CanvasNodeActionContextValue = {
    download?: (node: CanvasNodeData) => void;
    duplicate?: (node: CanvasNodeData) => void;
    deleteNode?: (node: CanvasNodeData) => void;
    /** 合并式更新节点 metadata；扩展节点在自己的面板里改参数时用。 */
    updateMetadata?: (nodeId: string, patch: CanvasNodeMetadata) => void;
    /** 改节点宽高；图片首次量到真实尺寸后按比例校正节点用。 */
    resizeNode?: (nodeId: string, size: { width: number; height: number }) => void;
};

export const CanvasNodeActionContext = createContext<CanvasNodeActionContextValue>({});

export function useCanvasNodeActions() {
    return useContext(CanvasNodeActionContext);
}
