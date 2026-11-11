import { describe, expect, test } from "bun:test";

import type { GenerationTask } from "@oc/services/api/task-center";
import { generationTaskNodeId, pickRecoveredGenerationTask } from "./canvas-generation-task-sync";

function task(partial: Partial<GenerationTask> & Pick<GenerationTask, "id">): GenerationTask {
    return {
        type: "canvas_video",
        status: "running",
        prompt: "",
        attempts: 1,
        createdAt: "2026-10-27T00:00:00.000Z",
        updatedAt: "2026-10-27T00:00:00.000Z",
        ...partial,
    };
}

describe("pickRecoveredGenerationTask", () => {
    test("reattaches a refreshed video node by server node id when the document lost taskId", () => {
        const tasks = [
            task({ id: "newer", nodeId: "node-video", updatedAt: "2026-10-27T00:02:00.000Z" }),
            task({ id: "older", nodeId: "node-video", updatedAt: "2026-10-27T00:01:00.000Z" }),
            task({ id: "other", nodeId: "node-other" }),
        ];
        expect(pickRecoveredGenerationTask("node-video", undefined, tasks)?.id).toBe("newer");
        expect(pickRecoveredGenerationTask("missing", undefined, tasks)).toBeUndefined();
    });

    test("keeps the task id already stored on the node", () => {
        const tasks = [
            task({ id: "newer", nodeId: "node-video" }),
            task({ id: "bound", nodeId: "node-video" }),
        ];
        expect(pickRecoveredGenerationTask("node-video", "bound", tasks)?.id).toBe("bound");
    });

    test("reads a node id embedded in the task input", () => {
        const embedded = task({
            id: "embedded",
            inputJson: JSON.stringify({ metadata: { nodeId: "node-video" } }),
        });
        expect(generationTaskNodeId(embedded)).toBe("node-video");
        expect(pickRecoveredGenerationTask("node-video", undefined, [embedded])?.id).toBe("embedded");
    });
});
