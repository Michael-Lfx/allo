import type { Dispatch, SetStateAction } from "react";

import type { NodeGenerationContext } from "@oc/components/canvas/canvas-node-generation";
import type { GenerationTask } from "@oc/services/api/task-center";
import type { AiConfig } from "@oc/stores/use-config-store";
import type { CanvasConnection, CanvasNodeData } from "@oc/types/canvas";

export type CanvasGenerationExecutorDependencies = {
    projectId: string;
    setNodes: Dispatch<SetStateAction<CanvasNodeData[]>>;
    setConnections: Dispatch<SetStateAction<CanvasConnection[]>>;
    setSelectedNodeIds: Dispatch<SetStateAction<Set<string>>>;
    setSelectedConnectionId: Dispatch<SetStateAction<string | null>>;
    setDialogNodeId: Dispatch<SetStateAction<string | null>>;
    startGenerationRequest: (targetNodeId: string, originNodeId: string, runningId?: string, controller?: AbortController) => AbortController;
    finishGenerationRequest: (targetNodeId: string, controller: AbortController) => void;
    bindGenerationTask: (targetNodeId: string, task: GenerationTask) => void;
    showError: (content: string) => void;
};

export type CanvasGenerationExecution = CanvasGenerationExecutorDependencies & {
    nodeId: string;
    sourceNode: CanvasNodeData | undefined;
    canvasNodes: CanvasNodeData[];
    canvasConnections: CanvasConnection[];
    prompt: string;
    effectivePrompt: string;
    generationConfig: AiConfig;
    generationContext: NodeGenerationContext;
    controller: AbortController;
    editingTextNode: boolean;
    registerPendingNodeIds: (nodeIds: string[]) => void;
};
