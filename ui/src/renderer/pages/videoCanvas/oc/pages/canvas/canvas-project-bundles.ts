import type { Dispatch, SetStateAction } from "react";
import type { CanvasAgentMode } from "@oc/components/canvas/canvas-agent-chat-ui";
import type { useCanvasAgentOperations } from "./use-canvas-agent-operations";
import type { useCanvasAssistantVisibility } from "./use-canvas-assistant-visibility";
import type { useCanvasHistory } from "./use-canvas-history";
import type { useCanvasRenderModel } from "./use-canvas-render-model";

/** useCanvasRenderModel 的返回形状：跨画布区块组件共享的渲染模型对象。 */
export type CanvasRenderModel = ReturnType<typeof useCanvasRenderModel>;

/** 画布历史相关动作束：{ historyState, undoCanvas, redoCanvas }。 */
export type CanvasHistoryActions = {
    historyState: ReturnType<typeof useCanvasHistory>["historyState"];
    undoCanvas: ReturnType<typeof useCanvasHistory>["undoCanvas"];
    redoCanvas: ReturnType<typeof useCanvasHistory>["redoCanvas"];
};

/** 智能体操作回调束：{ agentSnapshot, agentUndoCount, applyAgentOps, canUndoAgentOps, undoAgentOps, lastAgentChange, viewLastAgentChange, dismissLastAgentChange }。 */
export type CanvasAgentOps = {
    agentSnapshot: ReturnType<typeof useCanvasAgentOperations>["agentSnapshot"];
    agentUndoCount: number;
    applyAgentOps: ReturnType<typeof useCanvasAgentOperations>["applyAgentOps"];
    canUndoAgentOps: boolean;
    undoAgentOps: ReturnType<typeof useCanvasAgentOperations>["undoAgentOps"];
    lastAgentChange: ReturnType<typeof useCanvasAgentOperations>["lastAgentChange"];
    viewLastAgentChange: ReturnType<typeof useCanvasAgentOperations>["viewLastAgentChange"];
    dismissLastAgentChange: ReturnType<typeof useCanvasAgentOperations>["dismissLastAgentChange"];
};

/** 助手面板可见性状态束：{ agentMode, assistantClosing, assistantMounted, assistantOpen, closeAgent, openAgent, setAgentMode }。 */
export type CanvasAssistantState = {
    agentMode: CanvasAgentMode;
    assistantClosing: boolean;
    assistantMounted: boolean;
    assistantOpen: boolean;
    closeAgent: () => void;
    openAgent: ReturnType<typeof useCanvasAssistantVisibility>["openAgent"];
    setAgentMode: Dispatch<SetStateAction<CanvasAgentMode>>;
};
