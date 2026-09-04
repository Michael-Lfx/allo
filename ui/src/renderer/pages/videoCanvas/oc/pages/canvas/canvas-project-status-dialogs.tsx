import { useTranslation } from "react-i18next";
import { Button, Image, Modal } from "antd";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasNodeDisplayUrl } from "@oc/lib/canvas/canvas-media-id";
import { TaskDetailItem, taskStatusText } from "./canvas-project-feedback";
import type { GenerationTask, TaskLog } from "@oc/services/api/task-center";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";
import { VideoPlayer } from "@oc/components/video-player";
import { modelDisplayName, useEffectiveConfig } from "@oc/stores/use-config-store";

type CanvasProjectStatusDialogsProps = {
    theme: { node: { stroke: string; panel: string; muted: string; fill: string } };
    task: GenerationTask | null;
    taskLogs: TaskLog[];
    taskLoading: boolean;
    onCloseTask: () => void;
    superResolveNode: CanvasNodeData | null;
    onCloseSuperResolve: () => void;
    previewNode: CanvasNodeData | null;
    onClosePreview: () => void;
    clearConfirmOpen: boolean;
    onCancelClear: () => void;
    onConfirmClear: () => void;
};

export function CanvasProjectStatusDialogs({ theme, task, taskLogs, taskLoading, superResolveNode, previewNode, clearConfirmOpen, onCloseTask, onCloseSuperResolve, onClosePreview, onCancelClear, onConfirmClear }: CanvasProjectStatusDialogsProps) {
    useTranslation();
    const config = useEffectiveConfig();
    return (
        <>
            <Modal title={canvasT("videoCanvas.dialog.taskDetails", "任务详情")} open={Boolean(task)} footer={null} width={760} onCancel={onCloseTask}>
                {task ? (
                    <div className="space-y-4 text-sm">
                        <div className="grid grid-cols-2 gap-3 rounded-lg border p-3" style={{ borderColor: theme.node.stroke, background: theme.node.panel }}>
                            <TaskDetailItem label={canvasT("videoCanvas.dialog.currentStage", "当前阶段")} value={task.stage || taskStatusText(task.status)} />
                            <TaskDetailItem label={canvasT("videoCanvas.dialog.progress", "进度")} value={`${task.progress ?? 0}%`} />
                            <TaskDetailItem label={canvasT("videoCanvas.dialog.model", "模型")} value={task.model ? modelDisplayName(config, task.model) : canvasT("videoCanvas.dialog.defaultModel", "默认模型")} />
                            <TaskDetailItem label={canvasT("videoCanvas.dialog.taskId", "任务 ID")} value={task.id} />
                        </div>
                        <div>
                            <div className="mb-2 text-xs font-semibold" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.prompt", "提示词")}</div>
                            <div className="max-h-32 overflow-y-auto whitespace-pre-wrap rounded-lg p-3 text-xs leading-5" style={{ background: theme.node.fill }}>{task.prompt || canvasT("videoCanvas.dialog.notRecorded", "未记录")}</div>
                        </div>
                        <div>
                            <div className="mb-2 text-xs font-semibold" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.dialog.taskLogs", "任务日志")}</div>
                            <pre className="max-h-64 overflow-auto rounded-lg bg-neutral-950 p-3 text-[var(--fs-label)] leading-5 text-neutral-100">{taskLoading ? canvasT("videoCanvas.dialog.loading", "加载中...") : taskLogs.length ? taskLogs.map((log) => `[${new Date(log.createdAt).toLocaleString()}] ${log.level.toUpperCase()} ${log.message}`).join("\n") : canvasT("videoCanvas.dialog.noLogs", "暂无日志")}</pre>
                        </div>
                    </div>
                ) : null}
            </Modal>

            <Modal title={canvasT("videoCanvas.dialog.aiSuperResolve", "AI 超分")} open={Boolean(superResolveNode?.metadata?.content)} centered footer={null} onCancel={onCloseSuperResolve}>
                <div className="py-8 text-center text-base font-medium">{canvasT("videoCanvas.dialog.notImplemented", "暂未实现")}</div>
            </Modal>

            <Modal title={canvasT("videoCanvas.dialog.videoPreview", "视频预览")} open={Boolean(previewNode?.type === CanvasNodeType.Video && canvasNodeDisplayUrl(previewNode))} centered onCancel={onClosePreview} footer={null} width="min(1200px, calc(100vw - 32px))" styles={{ body: { padding: 0, display: "flex", justifyContent: "center", alignItems: "center", maxHeight: "84vh", overflow: "hidden", background: "#090909" } }}>
                {previewNode?.type === CanvasNodeType.Video && canvasNodeDisplayUrl(previewNode) ? <VideoPlayer src={canvasNodeDisplayUrl(previewNode)} mimeType={previewNode.metadata?.mimeType} title={previewNode.title || canvasT("videoCanvas.dialog.videoPreview", "视频预览")} className="max-h-[84vh] max-w-full bg-black" /> : null}
            </Modal>

            {previewNode?.type === CanvasNodeType.Image && canvasNodeDisplayUrl(previewNode) ? (
                <Image
                    src={canvasNodeDisplayUrl(previewNode)}
                    alt={previewNode.title || canvasT("videoCanvas.dialog.image", "图片")}
                    style={{ display: "none" }}
                    preview={{
                        open: true,
                        movable: true,
                        minScale: 0.5,
                        maxScale: 12,
                        scaleStep: 0.25,
                        onOpenChange: (open) => !open && onClosePreview(),
                    }}
                />
            ) : null}

            <Modal
                title={canvasT("videoCanvas.dialog.clearCanvasTitle", "清空画布？")}
                open={clearConfirmOpen}
                centered
                onCancel={onCancelClear}
                footer={<><Button onClick={onCancelClear}>{canvasT("videoCanvas.dialog.cancel", "取消")}</Button><Button danger type="primary" onClick={onConfirmClear}>{canvasT("videoCanvas.dialog.clear", "清空")}</Button></>}
            >
                <p className="text-sm opacity-60">{canvasT("videoCanvas.dialog.clearCanvasBody", "这会删除当前画布上的所有节点和连线。")}</p>
            </Modal>
        </>
    );
}
