import type { GenerationTask, TaskStatus } from "@oc/services/api/task-center";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";

export function getStatusLabel(status: TaskStatus): string {
    const map: Record<TaskStatus, string> = {
        queued: canvasT("videoCanvas.tasks.statusQueued", "排队中"),
        running: canvasT("videoCanvas.tasks.statusRunning", "生成中"),
        succeeded: canvasT("videoCanvas.tasks.statusSucceeded", "已完成"),
        failed: canvasT("videoCanvas.tasks.statusFailed", "失败"),
        cancelled: canvasT("videoCanvas.tasks.statusCancelled", "已取消"),
    };
    return map[status];
}

/** Indexed access re-resolves labels so language switches stay live. */
export const statusLabel = new Proxy({} as Record<TaskStatus, string>, {
    get(_target, prop: string | symbol) {
        if (typeof prop !== "string") return undefined;
        return getStatusLabel(prop as TaskStatus);
    },
});

export function getOperationLabel(value: string): string | undefined {
    const options = [
        { label: canvasT("videoCanvas.tasks.opAgentSession", "Agent 会话：拆解影视工作流"), value: "agent_session" },
        { label: canvasT("videoCanvas.tasks.opTextToVideo", "文生视频"), value: "text_to_video" },
        { label: canvasT("videoCanvas.tasks.opImageToVideo", "图生视频"), value: "image_to_video" },
        { label: canvasT("videoCanvas.tasks.opExtend", "视频续写"), value: "extend" },
        { label: canvasT("videoCanvas.tasks.opInpaint", "视频局部修改"), value: "inpaint" },
        { label: canvasT("videoCanvas.tasks.opReplace", "元素替换"), value: "replace_element" },
        { label: canvasT("videoCanvas.tasks.opCamera", "镜头/运镜调整"), value: "camera_motion" },
        { label: canvasT("videoCanvas.tasks.opStyle", "风格迁移"), value: "style_transfer" },
        { label: canvasT("videoCanvas.tasks.opAudioToVideo", "参考音频生成视频"), value: "audio_to_video" },
        { label: canvasT("videoCanvas.tasks.opCompare", "结果版本对比"), value: "compare_versions" },
    ];
    return options.find((item) => item.value === value)?.label;
}

export function getTaskTypeLabel(type: string): string | undefined {
    const map: Record<string, string> = {
        agent_session: canvasT("videoCanvas.tasks.typeAgentSession", "Agent 会话"),
        agent_storyboard: canvasT("videoCanvas.tasks.typeAgentStoryboard", "Agent 分镜"),
        agent_storyboard_rows: canvasT("videoCanvas.tasks.typeAgentStoryboardRows", "分镜脚本"),
        canvas_image: canvasT("videoCanvas.tasks.typeCanvasImage", "画布生图"),
        canvas_video: canvasT("videoCanvas.tasks.typeCanvasVideo", "画布视频"),
        canvas_audio: canvasT("videoCanvas.tasks.typeCanvasAudio", "画布音频"),
        canvas_text: canvasT("videoCanvas.tasks.typeCanvasText", "画布文本"),
    };
    return map[type];
}

export function formatTaskKind(task: GenerationTask) {
    if (task.type === "agent_session" || task.operation === "agent_session") {
        return canvasT("videoCanvas.tasks.typeAgentSession", "Agent 会话");
    }

    const typeLabel = getTaskTypeLabel(task.type);
    const operationLabel = task.operation ? getOperationLabel(task.operation) : "";

    if (task.type === "canvas_video" && operationLabel) {
        return canvasT("videoCanvas.tasks.kindWithOp", "{{type}} · {{op}}", {
            type: typeLabel || canvasT("videoCanvas.tasks.typeCanvasVideo", "画布视频"),
            op: operationLabel,
        });
    }
    if (typeLabel) return typeLabel;
    if (operationLabel) return operationLabel;
    if (task.type.startsWith("video_")) return canvasT("videoCanvas.tasks.videoTask", "视频任务");
    return canvasT("videoCanvas.tasks.title", "生成任务");
}
