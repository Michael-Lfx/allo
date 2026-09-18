import { canvasPlaybookId } from "./catalog";
import type { CanvasNodeData } from "@oc/types/canvas";

export function canvasVideoSessionProps(
    projectId: string,
    nodes: Pick<CanvasNodeData, "metadata">[],
    extra?: Record<string, string | number | boolean | null>,
) {
    const playbookId = canvasPlaybookId(nodes);
    return {
        feature: "video_generation" as const,
        workflow: "canvas",
        session_id: projectId,
        project_id: projectId,
        ...(playbookId ? { playbook_id: playbookId } : {}),
        ...extra,
    };
}
