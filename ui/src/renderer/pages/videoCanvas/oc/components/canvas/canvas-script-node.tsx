import { useTranslation } from "react-i18next";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { Button, Input, InputNumber, Popover, Segmented, Select, Tooltip } from "antd";
import { Clapperboard, Expand, Grid3X3, ListTree, Merge, Minus, Plus, RefreshCw, Send, Square, Trash2, X } from "lucide-react";

import { CanvasResourceMentionTextarea } from "@oc/components/canvas/canvas-resource-mention-textarea";
import { ModelPicker } from "@oc/components/model-picker";
import { buildGenerationConfig } from "@oc/lib/canvas/canvas-project-generation";
import type { CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import { pipelineStatusLabel, type CanvasStoryboardPipelineProgress, type StoryboardPipelineStage } from "@oc/lib/canvas/canvas-storyboard-progress";
import { isContentModerationError } from "@oc/lib/generation-error";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { navigateToSettings } from "@oc/lib/settings-navigation";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { useEffectiveConfig } from "@oc/stores/use-config-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { CanvasGenerationBatch, CanvasGenerationBatchItem, CanvasGenerationBatchItemStatus, CanvasNodeData, CanvasNodeStatus, CanvasWorkspaceMode, StoryboardRow, StoryboardShotCount, StoryboardShotDuration, StoryboardVideoInputMode } from "@oc/types/canvas";

export const STORYBOARD_ROW_HEIGHT = 48;
export const STORYBOARD_HEADER_HEIGHT = 124;
const STORYBOARD_ADD_ROW_HEIGHT = 36;
const STORYBOARD_COMPOSER_MIN_HEIGHT = 104;
const STORYBOARD_COMPOSER_MAX_HEIGHT = 180;
const STORYBOARD_PROMPT_MIN_HEIGHT = 40;
const STORYBOARD_PROMPT_MAX_HEIGHT = 116;
const SCRIPT_GRID_TEMPLATE = "72px 150px minmax(280px, 1.4fr) minmax(220px, 1fr) 58px";

export function storyboardNodeHeight(rowCount: number, composerHeight = STORYBOARD_COMPOSER_MIN_HEIGHT) {
    const visibleRows = Math.min(Math.max(rowCount, 1), 4);
    return STORYBOARD_HEADER_HEIGHT + visibleRows * STORYBOARD_ROW_HEIGHT + STORYBOARD_ADD_ROW_HEIGHT + Math.min(STORYBOARD_COMPOSER_MAX_HEIGHT, Math.max(STORYBOARD_COMPOSER_MIN_HEIGHT, composerHeight));
}

export function storyboardMinNodeHeight(composerHeight = STORYBOARD_COMPOSER_MIN_HEIGHT) {
    return STORYBOARD_HEADER_HEIGHT + STORYBOARD_ROW_HEIGHT + STORYBOARD_ADD_ROW_HEIGHT + Math.min(STORYBOARD_COMPOSER_MAX_HEIGHT, Math.max(STORYBOARD_COMPOSER_MIN_HEIGHT, composerHeight));
}

export function storyboardTableHeight(nodeHeight: number, composerHeight = STORYBOARD_COMPOSER_MIN_HEIGHT) {
    return Math.max(STORYBOARD_ROW_HEIGHT, nodeHeight - STORYBOARD_HEADER_HEIGHT - STORYBOARD_ADD_ROW_HEIGHT - Math.min(STORYBOARD_COMPOSER_MAX_HEIGHT, Math.max(STORYBOARD_COMPOSER_MIN_HEIGHT, composerHeight)));
}

export function CanvasScriptNodeContent({ node, batch, pipeline, mentionReferences, onOpen, onCreateImageNodes, onCreateVideoNodes, onGenerateImages, onGenerateVideos, onVideoInputModeChange, onMergeVideos, onCreateActionBoards, onRetryBatch, onRetryBatchItem, onStopBatch, onCancelBatchItem, onAddRow, onRemoveRow, onUpdateRow, onPromptChange, onGenerateScript, onModelChange, onShotDurationChange, onShotCountChange, onComposerHeightChange, onConnectStart, onScrollTopChange }: {
    node: CanvasNodeData;
    batch?: CanvasGenerationBatch;
    pipeline: CanvasStoryboardPipelineProgress;
    mentionReferences: CanvasResourceReference[];
    onOpen: () => void;
    onCreateImageNodes: () => void;
    onCreateVideoNodes: () => void;
    onGenerateImages: () => void;
    onGenerateVideos: () => void;
    onVideoInputModeChange: (mode: StoryboardVideoInputMode) => void;
    onMergeVideos: () => void;
    onCreateActionBoards: () => void;
    onRetryBatch: (batchId: string) => void;
    onRetryBatchItem: (batchId: string, itemId: string) => void;
    onStopBatch: (batchId: string) => void;
    onCancelBatchItem: (batchId: string, itemId: string) => void;
    onAddRow: () => void;
    onRemoveRow: (rowId: string) => void;
    onUpdateRow: (rowId: string, patch: Partial<StoryboardRow>) => void;
    onPromptChange: (prompt: string) => void;
    onGenerateScript: (prompt: string) => void;
    onModelChange: (model: string) => void;
    onShotDurationChange: (duration: StoryboardShotDuration) => void;
    onShotCountChange: (count: StoryboardShotCount) => void;
    onComposerHeightChange: (height: number) => void;
    onConnectStart: (event: ReactPointerEvent, rowId: string, handleType: "source" | "target") => void;
    onScrollTopChange: (scrollTop: number) => void;
    workspaceMode?: CanvasWorkspaceMode;
}) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const effectiveConfig = useEffectiveConfig();
    const generationConfig = buildGenerationConfig(effectiveConfig, node, "text");
    const rows = node.metadata?.storyboard?.rows || [];
    const [prompt, setPrompt] = useState(node.metadata?.composerContent || "");
    // 滚动热路径：scrollTop 只进 ref，端口位置由 rAF 里直改 DOM transform，
    // 不再以滚动频率触发本组件与 WorldLayers 的 React 重渲；store 写入同样合帧。
    const scrollTopRef = useRef(0);
    const portsInnerRef = useRef<HTMLDivElement | null>(null);
    const scrollFrameRef = useRef<number | null>(null);
    const notifiedScrollRef = useRef(0);
    const composerHeightChangeRef = useRef(onComposerHeightChange);
    const reportedComposerHeightRef = useRef<number | null>(null);
    const composerHeight = node.metadata?.storyboardComposerHeight || STORYBOARD_COMPOSER_MIN_HEIGHT;
    const tableHeight = storyboardTableHeight(node.height, composerHeight);
    const totalDuration = rows.reduce((sum, row) => sum + (Number(row.durationSeconds) || 0), 0);
    const shotDuration = node.metadata?.storyboardShotDuration || "auto";
    const shotCount = node.metadata?.storyboardShotCount || "auto";
    const videoInputMode = node.metadata?.storyboardVideoInputMode || "direct";
    const batchItemByRowId = useMemo(() => new Map((batch?.items || []).map((item) => [item.rowId, item])), [batch?.items]);
    const batchSummary = batch ? generationBatchSummary(batch) : null;
    const hasFailedBatchItems = Boolean(batch?.items.some((item) => item.status === "failed"));
    const hasWaitingBatchItems = Boolean(batch?.items.some((item) => item.status === "waiting" || item.status === "submitting"));
    const hasActiveBatchItems = Boolean(batch?.items.some((item) => item.status === "waiting" || item.status === "submitting" || item.status === "queued" || item.status === "running"));
    const taskFeedback = node.metadata?.status === "loading"
        ? `${node.metadata.taskStage || canvasT("videoCanvas.script.creatingTask", "正在创建任务")}${typeof node.metadata.taskProgress === "number" ? ` · ${node.metadata.taskProgress}%` : ""}`
        : node.metadata?.status === "error" ? formatCanvasUserError(node.metadata.errorDetails) : "";
    const submitPrompt = () => {
        const value = prompt.trim();
        if (value && node.metadata?.status !== "loading") onGenerateScript(value);
    };
    useLayoutEffect(() => {
        composerHeightChangeRef.current = onComposerHeightChange;
    }, [onComposerHeightChange]);
    const resizePrompt = useCallback((contentHeight: number) => {
        const promptHeight = Math.min(STORYBOARD_PROMPT_MAX_HEIGHT, Math.max(STORYBOARD_PROMPT_MIN_HEIGHT, contentHeight));
        const composerHeight = promptHeight + 64;
        const previous = reportedComposerHeightRef.current;
        if (previous !== null && Math.abs(previous - composerHeight) < 1) return;
        reportedComposerHeightRef.current = composerHeight;
        composerHeightChangeRef.current(composerHeight);
    }, []);
    useEffect(() => () => {
        if (scrollFrameRef.current !== null) cancelAnimationFrame(scrollFrameRef.current);
    }, []);

    return (
        <div className="relative flex h-full w-full flex-col overflow-visible" style={{ color: theme.node.text }} onDoubleClick={(event) => event.stopPropagation()}>
            <div className="relative flex h-10 shrink-0 items-center gap-2 rounded-t-[17px] border-b px-4" style={{ borderColor: theme.node.stroke, background: theme.node.panel }}>
                <Clapperboard className="size-4" />
                <span className="min-w-0 flex-1 truncate text-sm font-semibold" title={node.title || canvasT("videoCanvas.node.script", "分镜脚本")}>{node.title || canvasT("videoCanvas.node.script", "分镜脚本")}</span>
                {batchSummary ? <span className="min-w-0 max-w-[42%] truncate text-[var(--fs-label)] font-medium" title={batchSummary} style={{ color: batch?.status === "partial_failed" ? theme.accent.danger : theme.node.muted }}>{batchSummary}</span> : taskFeedback ? <span className="min-w-0 max-w-[38%] truncate text-[var(--fs-label)] font-medium" title={taskFeedback} style={{ color: node.metadata?.status === "error" ? theme.accent.danger : theme.node.muted }}>{taskFeedback}</span> : null}
                <span className="text-[var(--fs-caption)] font-semibold tabular-nums" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.script.shotsDuration", "{{count}} 镜 · {{seconds}}s", { count: rows.length, seconds: totalDuration })}</span>
                {batch ? <>
                    {hasFailedBatchItems ? <Tooltip title={canvasT("videoCanvas.script.retryFailed", "重试失败项")}><button type="button" className="grid size-7 place-items-center rounded outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ "--tw-ring-color": theme.node.muted } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onRetryBatch(batch.id); }} aria-label={canvasT("videoCanvas.script.retryFailed", "重试失败项")}><RefreshCw className="size-3.5" /></button></Tooltip> : null}
                    {hasWaitingBatchItems ? <Tooltip title={canvasT("videoCanvas.script.stopRemaining", "停止剩余任务")}><button type="button" className="grid size-7 place-items-center rounded outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ "--tw-ring-color": theme.node.muted } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onStopBatch(batch.id); }} aria-label={canvasT("videoCanvas.script.stopRemaining", "停止剩余任务")}><Square className="size-3.5" /></button></Tooltip> : null}
                    <Popover placement="bottomRight" trigger="click" content={<GenerationBatchDetails batch={batch} rows={rows} onRetryItem={(itemId) => onRetryBatchItem(batch.id, itemId)} onCancelItem={(itemId) => onCancelBatchItem(batch.id, itemId)} />}><Tooltip title={canvasT("videoCanvas.script.viewDetails", "查看详情")}><button type="button" className="grid size-7 place-items-center rounded outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ "--tw-ring-color": theme.node.muted } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()} aria-label={canvasT("videoCanvas.script.batchDetailsAria", "查看批次详情")}><ListTree className="size-3.5" /></button></Tooltip></Popover>
                </> : null}
                <Tooltip title={canvasT("videoCanvas.script.actionBoard", "生成动作拆分 12 宫格")}><button type="button" disabled={!rows.length || hasActiveBatchItems} className="grid size-7 place-items-center rounded outline-none transition hover:bg-black/5 focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-30 dark:hover:bg-white/10" style={{ "--tw-ring-color": theme.node.muted } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onCreateActionBoards(); }}><Grid3X3 className="size-3.5" /></button></Tooltip>
                <Tooltip title={canvasT("videoCanvas.script.fullscreenEdit", "全屏编辑")}><button type="button" className="grid size-7 place-items-center rounded outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ "--tw-ring-color": theme.node.muted } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onOpen(); }}><Expand className="size-3.5" /></button></Tooltip>
            </div>
            <StoryboardPipelineBar
                pipeline={pipeline}
                disabled={!rows.length || node.metadata?.status === "loading" || hasActiveBatchItems}
                theme={theme}
                onCreateImageNodes={onCreateImageNodes}
                onCreateVideoNodes={onCreateVideoNodes}
                onGenerateImages={onGenerateImages}
                onGenerateVideos={onGenerateVideos}
                videoInputMode={videoInputMode}
                onVideoInputModeChange={onVideoInputModeChange}
                onMergeVideos={onMergeVideos}
            />
            <div className="grid h-9 shrink-0 items-center border-b text-xs font-semibold" style={{ borderColor: theme.node.stroke, color: theme.node.muted, gridTemplateColumns: SCRIPT_GRID_TEMPLATE }}>
                <HeaderCell borderColor={theme.node.stroke} align="center">{canvasT("videoCanvas.script.colIndex", "序号")}</HeaderCell>
                <HeaderCell borderColor={theme.node.stroke} align="center">{canvasT("videoCanvas.script.colDuration", "时长")}</HeaderCell>
                <HeaderCell borderColor={theme.node.stroke}>{canvasT("videoCanvas.script.colVisual", "画面描述")}</HeaderCell>
                <HeaderCell borderColor={theme.node.stroke}>{canvasT("videoCanvas.script.colDialogue", "台词/旁白")}</HeaderCell>
                <span className="text-center">{canvasT("videoCanvas.script.colActions", "操作")}</span>
            </div>
            <div
                data-canvas-wheel-scroll
                tabIndex={0}
                role="region"
                aria-label={canvasT("videoCanvas.script.shotListAria", "分镜镜头列表")}
                className="storyboard-scrollbar min-h-0 flex-1 overflow-y-scroll overflow-x-hidden outline-none focus-visible:ring-1 focus-visible:ring-inset"
                style={{ "--tw-ring-color": theme.node.muted } as CSSProperties}
                onScroll={(event) => {
                    const next = event.currentTarget.scrollTop;
                    scrollTopRef.current = next;
                    if (scrollFrameRef.current !== null) return;
                    scrollFrameRef.current = requestAnimationFrame(() => {
                        scrollFrameRef.current = null;
                        if (portsInnerRef.current) portsInnerRef.current.style.transform = `translateY(${-scrollTopRef.current}px)`;
                        if (notifiedScrollRef.current !== scrollTopRef.current) {
                            notifiedScrollRef.current = scrollTopRef.current;
                            onScrollTopChange(scrollTopRef.current);
                        }
                    });
                }}
                onWheel={(event) => event.stopPropagation()}
            >
                {rows.length ? rows.map((row) => (
                    <div key={row.id} className="relative grid border-b" style={{ height: STORYBOARD_ROW_HEIGHT, borderColor: theme.node.stroke, gridTemplateColumns: SCRIPT_GRID_TEMPLATE }}>
                        <div className="flex flex-col items-center justify-center border-r tabular-nums" style={{ color: theme.node.muted, borderColor: theme.node.stroke }}><span className="text-sm">{row.shotNumber}</span>{batchItemByRowId.get(row.id) ? <span className="max-w-16 truncate text-[var(--fs-micro)] leading-3" title={generationBatchItemLabel(batchItemByRowId.get(row.id)!)}>{generationBatchItemLabel(batchItemByRowId.get(row.id)!)}</span> : null}</div>
                        <div className="grid grid-cols-[32px_1fr_32px] items-center border-r px-2" style={{ borderColor: theme.node.stroke }}>
                            <SmallButton title={canvasT("videoCanvas.script.durationMinus", "减少 1 秒")} onClick={() => onUpdateRow(row.id, { durationSeconds: Math.max(1, row.durationSeconds - 1) })}><Minus className="size-3" /></SmallButton>
                            <span className="text-center text-sm font-medium tabular-nums">{row.durationSeconds}s</span>
                            <SmallButton title={canvasT("videoCanvas.script.durationPlus", "增加 1 秒")} onClick={() => onUpdateRow(row.id, { durationSeconds: Math.min(60, row.durationSeconds + 1) })}><Plus className="size-3" /></SmallButton>
                        </div>
                        <CompactInput value={row.plotDescription} placeholder={canvasT("videoCanvas.script.visualPlaceholder", "描述画面内容")} onChange={(value) => onUpdateRow(row.id, { plotDescription: value })} borderColor={theme.node.stroke} />
                        <CompactInput value={row.dialogue} placeholder={canvasT("videoCanvas.script.dialoguePlaceholder", "台词或旁白")} onChange={(value) => onUpdateRow(row.id, { dialogue: value })} borderColor={theme.node.stroke} />
                        <div className="grid h-full place-items-center">
                            <button type="button" disabled={rows.length <= 1} className="grid size-7 place-items-center rounded outline-none opacity-55 transition enabled:hover:bg-red-500/10 enabled:hover:opacity-100 focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-20" style={{ color: theme.accent.danger, "--tw-ring-color": theme.accent.danger } as CSSProperties} title={rows.length <= 1 ? canvasT("videoCanvas.script.keepOneShot", "至少保留一个镜头") : canvasT("videoCanvas.script.deleteShot", "删除镜头")} aria-label={canvasT("videoCanvas.script.deleteShotAria", "删除镜头 {{n}}", { n: row.shotNumber })} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onRemoveRow(row.id); }}><Trash2 className="size-3.5" /></button>
                        </div>
                    </div>
                )) : <button type="button" className="grid h-full min-h-24 w-full place-items-center text-sm" style={{ color: theme.node.muted }} onClick={(event) => { event.stopPropagation(); onAddRow(); }}>{canvasT("videoCanvas.script.addFirstShot", "+ 添加第一个镜头")}</button>}
            </div>
            <div className="flex h-9 shrink-0 items-center justify-center border-b" style={{ borderColor: theme.node.stroke, background: theme.node.panel }}>
                <button type="button" className="inline-flex h-7 items-center gap-1 rounded px-2 text-xs font-medium outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ "--tw-ring-color": theme.node.muted } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onAddRow(); }}><Plus className="size-3.5" />{canvasT("videoCanvas.script.addRow", "添加行")}</button>
            </div>
            <div className="relative grid shrink-0 grid-rows-[minmax(0,1fr)_1.75rem] gap-1.5 rounded-b-[17px] p-2.5" style={{ height: composerHeight, background: theme.node.panel }}>
                <CanvasResourceMentionTextarea
                    rows={1}
                    references={mentionReferences}
                    aria-label={canvasT("videoCanvas.script.plotSettingsAria", "分镜剧情与项目设定")}
                    containerClassName="h-full min-h-0 overflow-hidden"
                    className="thin-scrollbar h-full min-h-0 w-full touch-pan-y resize-none overflow-y-auto overflow-x-hidden overscroll-contain rounded-md border bg-transparent px-3 py-2 text-sm leading-5 outline-none transition placeholder:opacity-45 focus:ring-1"
                    style={{ borderColor: theme.node.stroke, color: theme.node.text, "--tw-ring-color": theme.node.muted } as CSSProperties}
                    value={prompt}
                    placeholder={canvasT("videoCanvas.script.composerPlaceholder", "描述想生成的脚本或视频内容")}
                    onContentSizeChange={resizePrompt}
                    onChange={(value) => {
                        setPrompt(value);
                        onPromptChange(value);
                    }}
                    onKeyDown={(event) => {
                        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                            event.preventDefault();
                            submitPrompt();
                        }
                    }}
                    onMouseDown={(event) => event.stopPropagation()}
                    onPointerDown={(event) => event.stopPropagation()}
                    onWheel={(event) => event.stopPropagation()}
                />
                <div className="flex h-7 min-w-0 items-center justify-end gap-1.5 overflow-hidden" onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
                    <Tooltip title={canvasT("videoCanvas.script.textModelHint", "脚本生成需要文本理解与结构化输出能力，仅展示文本模型；视频/图片模型无法生成分镜表")} placement="topLeft">
                        <div className="mr-auto min-w-0 max-w-[13rem] flex-1 overflow-hidden">
                            <ModelPicker
                                className="!h-7 !max-h-7 !w-full !min-w-0 !max-w-full !text-[var(--fs-tiny)] !font-normal [&_.canvas-model-picker-trigger-icon]:!size-5 [&_img]:!size-3 [&_.lucide]:!size-3"
                                fullWidth
                                variant="creation"
                                config={generationConfig}
                                value={generationConfig.model}
                                capability="text"
                                placeholder={canvasT("videoCanvas.script.selectTextModel", "选择文本模型")}
                                showSelectedPrice={false}
                                onChange={onModelChange}
                                onMissingConfig={() => navigateToSettings({ continueCreation: true })}
                            />
                        </div>
                    </Tooltip>
                    <Select<StoryboardShotCount>
                        className="w-[7.5rem] shrink-0"
                        size="small"
                        value={shotCount}
                        disabled={node.metadata?.status === "loading"}
                        options={[{ value: "auto", label: canvasT("videoCanvas.script.shotCountAuto", "分镜数量：自动拆分") }, ...Array.from({ length: 10 }, (_, index) => ({ value: String(index + 1) as StoryboardShotCount, label: canvasT("videoCanvas.script.shotCountN", "分镜数量：{{n}}", { n: index + 1 }) }))]}
                        popupMatchSelectWidth={false}
                        onChange={onShotCountChange}
                    />
                    <Select<StoryboardShotDuration>
                        className="w-[7.5rem] shrink-0"
                        size="small"
                        value={shotDuration}
                        disabled={node.metadata?.status === "loading"}
                        options={[
                            { value: "auto", label: canvasT("videoCanvas.script.lensAuto", "镜头：自动拆分") },
                            { value: "5", label: canvasT("videoCanvas.script.lens5s", "镜头：单个5S") },
                            { value: "10", label: canvasT("videoCanvas.script.lens10s", "镜头：单个10S") },
                            { value: "15", label: canvasT("videoCanvas.script.lens15s", "镜头：单个15S") },
                            { value: "30", label: canvasT("videoCanvas.script.lens30s", "镜头：单个30S") },
                        ]}
                        popupMatchSelectWidth={false}
                        onChange={onShotDurationChange}
                    />
                    <Button
                        size="small"
                        shape="circle"
                        className="!h-7 !w-7 !min-w-7 shrink-0"
                        icon={<Send className="size-3.5" />}
                        disabled={!prompt.trim() || node.metadata?.status === "loading"}
                        loading={node.metadata?.status === "loading"}
                        style={{ background: theme.toolbar.itemHover, borderColor: theme.node.stroke, color: theme.node.text }}
                        onMouseDown={(event) => event.stopPropagation()}
                        onClick={submitPrompt}
                    />
                </div>
                <RowHandle side="left" top={composerHeight / 2} tone="idle" theme={theme} title={canvasT("videoCanvas.script.connectContext", "连接文本节点作为项目设定")} onPointerDown={(event) => onConnectStart(event, "context", "target")} />
            </div>
            <div className="absolute inset-x-0 overflow-hidden" style={{ top: STORYBOARD_HEADER_HEIGHT, height: tableHeight - STORYBOARD_HEADER_HEIGHT }}>
                <div ref={portsInnerRef} className="absolute inset-x-0 top-0 will-change-transform">
                    {rows.map((row, index) => {
                        const top = index * STORYBOARD_ROW_HEIGHT + STORYBOARD_ROW_HEIGHT / 2;
                        return (
                            <div key={`ports-${row.id}`}>
                                <RowHandle side="left" top={top} tone={batchItemTone(batchItemByRowId.get(row.id)) || row.status} theme={theme} onPointerDown={(event) => onConnectStart(event, row.id, "target")} />
                                <RowHandle side="right" top={top} tone={batchItemTone(batchItemByRowId.get(row.id)) || row.status} theme={theme} onPointerDown={(event) => onConnectStart(event, row.id, "source")} />
                            </div>
                        );
                    })}
                </div>
            </div>
        </div>
    );
}

function StoryboardPipelineBar({ pipeline, disabled, theme, videoInputMode, onCreateImageNodes, onCreateVideoNodes, onGenerateImages, onGenerateVideos, onVideoInputModeChange, onMergeVideos }: {
    pipeline: CanvasStoryboardPipelineProgress;
    disabled: boolean;
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    videoInputMode: StoryboardVideoInputMode;
    onCreateImageNodes: () => void;
    onCreateVideoNodes: () => void;
    onGenerateImages: () => void;
    onGenerateVideos: () => void;
    onVideoInputModeChange: (mode: StoryboardVideoInputMode) => void;
    onMergeVideos: () => void;
}) {
    const missingImages = Math.max(0, pipeline.images.total - pipeline.images.created);
    const missingVideos = Math.max(0, pipeline.videos.total - pipeline.videos.created);
    const canMerge = pipeline.successfulVideoNodeIds.length >= 2 && pipeline.final.success === 0;
    return (
        <div className="grid h-12 shrink-0 grid-cols-3 border-b" style={{ borderColor: theme.node.stroke, background: theme.node.fill }} onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
            <PipelineStageCell label={videoInputMode === "keyframe" ? canvasT("videoCanvas.script.keyframe", "首帧") : canvasT("videoCanvas.script.storyboardImageOptional", "分镜图（可选）")} stage={pipeline.images} theme={theme}>
                <Button size="small" type="text" disabled={disabled || missingImages === 0} onClick={onCreateImageNodes}>{missingImages ? canvasT("videoCanvas.script.createImageNodes", "创建 {{count}} 个图片节点", { count: missingImages }) : canvasT("videoCanvas.script.imageNodesCreated", "图片节点已创建")}</Button>
                <Button size="small" type="text" disabled={disabled || pipeline.images.incomplete === 0} onClick={onGenerateImages}>{canvasT("videoCanvas.script.genIncompleteImages", "生成未完成的图片")}</Button>
            </PipelineStageCell>
            <PipelineStageCell label={canvasT("videoCanvas.script.shotVideos", "镜头视频")} stage={pipeline.videos} theme={theme}>
                <Segmented<StoryboardVideoInputMode>
                    size="small"
                    value={videoInputMode}
                    options={[{ value: "direct", label: canvasT("videoCanvas.script.directGen", "直接生成") }, { value: "keyframe", label: canvasT("videoCanvas.script.keyframeFirst", "先做首帧") }]}
                    onChange={onVideoInputModeChange}
                />
                <Button size="small" type="text" disabled={disabled || missingVideos === 0} onClick={onCreateVideoNodes}>{missingVideos ? canvasT("videoCanvas.script.createVideoNodes", "创建 {{count}} 个视频节点", { count: missingVideos }) : canvasT("videoCanvas.script.videoNodesCreated", "视频节点已创建")}</Button>
                <Button size="small" type="text" disabled={disabled || pipeline.videos.incomplete === 0} onClick={onGenerateVideos}>{videoInputMode === "keyframe" ? canvasT("videoCanvas.script.confirmKeyframeGen", "确认首帧并生成") : canvasT("videoCanvas.script.genIncompleteVideos", "生成未完成的视频")}</Button>
            </PipelineStageCell>
            <PipelineStageCell label={canvasT("videoCanvas.script.mergeFinal", "合并成片")} stage={pipeline.final} theme={theme} last>
                <button
                    type="button"
                    className="inline-flex h-7 max-w-full items-center justify-center gap-1.5 rounded-[var(--r-sm)] px-2 text-[var(--fs-tiny)] font-semibold transition-colors hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:hover:brightness-100"
                    style={{
                        background: canMerge ? theme.accent.primary : "transparent",
                        color: canMerge ? "var(--primary-foreground)" : theme.node.faint,
                        "--tw-ring-color": theme.accent.primary,
                        "--tw-ring-offset-color": theme.node.fill,
                    } as CSSProperties}
                    disabled={!canMerge}
                    onClick={onMergeVideos}
                >
                    <Merge className="size-3 shrink-0" />
                    {pipeline.final.success ? canvasT("videoCanvas.script.finalDone", "成片已完成") : pipeline.successfulVideoNodeIds.length >= 2 ? canvasT("videoCanvas.script.mergeNVideos", "合并 {{count}} 段视频", { count: pipeline.successfulVideoNodeIds.length }) : canvasT("videoCanvas.script.needTwoVideos", "至少完成 2 段视频")}
                </button>
            </PipelineStageCell>
        </div>
    );
}

function PipelineStageCell({ label, stage, theme, children, last = false }: { label: string; stage: StoryboardPipelineStage; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; children: ReactNode; last?: boolean }) {
    return (
        <div className={`flex min-w-0 items-center gap-2 px-3 ${last ? "" : "border-r"}`} style={{ borderColor: theme.node.stroke }}>
            <div className="min-w-[64px] shrink-0">
                <div className="text-[var(--fs-label)] font-semibold">{label}</div>
                <div className="text-[var(--fs-micro)] leading-3" style={{ color: stage.failed ? theme.accent.danger : theme.node.muted }}>{pipelineStatusLabel(stage)}</div>
            </div>
            <div className="flex min-w-0 flex-1 items-center justify-end gap-1 overflow-hidden [&_.ant-btn]:!h-7 [&_.ant-btn]:!px-2 [&_.ant-btn]:!text-[var(--fs-tiny)]">{children}</div>
        </div>
    );
}

function GenerationBatchDetails({ batch, rows, onRetryItem, onCancelItem }: { batch: CanvasGenerationBatch; rows: StoryboardRow[]; onRetryItem: (itemId: string) => void; onCancelItem: (itemId: string) => void }) {
    const shotByRowId = new Map(rows.map((row) => [row.id, row.shotNumber]));
    return <div className="w-80" onMouseDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}>
        <div className="mb-2 flex items-center justify-between gap-3"><span className="text-sm font-semibold">{canvasT("videoCanvas.script.batchDetails", "{{mode}}详情", { mode: generationBatchModeLabel(batch) })}</span><span className="text-xs text-foreground/50">{canvasT("videoCanvas.script.itemCount", "{{count}} 项", { count: batch.items.length })}</span></div>
        <div className="thin-scrollbar max-h-72 overflow-y-auto">
            {batch.items.map((item) => {
                const cancellable = Boolean(item.taskId && (item.status === "queued" || item.status === "running"));
                const requiresPromptChange = isContentModerationError(item.errorDetails);
                return <div key={item.id} className="flex min-h-9 items-center gap-2 border-t border-foreground/10 py-1.5 first:border-t-0">
                    <span className="w-14 shrink-0 text-xs font-medium">{canvasT("videoCanvas.script.shotN", "镜头 {{n}}", { n: shotByRowId.get(item.rowId) || "--" })}</span>
                    <span className="min-w-0 flex-1 truncate text-xs text-foreground/60" title={item.errorDetails ? formatCanvasUserError(item.errorDetails) : undefined}>{generationBatchItemLabel(item)}{item.retryCount ? canvasT("videoCanvas.script.retryN", " · 重试 {{n}}", { n: item.retryCount }) : ""}</span>
                    {item.status === "failed" ? <Tooltip title={requiresPromptChange ? canvasT("videoCanvas.script.retryNeedPrompt", "请先修改提示词，再重试这个镜头") : canvasT("videoCanvas.script.retryThisShot", "只重试这个镜头")}><button type="button" className="grid size-7 shrink-0 place-items-center rounded outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" onClick={() => onRetryItem(item.id)} aria-label={canvasT("videoCanvas.script.retryShotAria", "重试镜头 {{n}}", { n: shotByRowId.get(item.rowId) || "" })}><RefreshCw className="size-3.5" /></button></Tooltip> : null}
                    {cancellable ? <Tooltip title={canvasT("videoCanvas.script.cancelItem", "取消这个后台任务")}><button type="button" className="grid size-7 shrink-0 place-items-center rounded outline-none transition hover:bg-red-500/10 focus-visible:ring-2" onClick={() => onCancelItem(item.id)} aria-label={canvasT("videoCanvas.script.cancelShotAria", "取消镜头 {{n}} 任务", { n: shotByRowId.get(item.rowId) || "" })}><X className="size-3.5" /></button></Tooltip> : null}
                </div>;
            })}
        </div>
    </div>;
}

function generationBatchModeLabel(batch: CanvasGenerationBatch) {
    return batch.mode === "storyboard_video" ? canvasT("videoCanvas.script.modeVideo", "视频生成") : batch.mode === "storyboard_image" ? canvasT("videoCanvas.script.modeImage", "分镜图生成") : canvasT("videoCanvas.script.modeAction", "动作板生成");
}

function generationBatchSummary(batch: CanvasGenerationBatch) {
    const count = (status: CanvasGenerationBatchItemStatus) => batch.items.filter((item) => item.status === status).length;
    const generating = count("submitting") + count("queued") + count("running");
    const stopped = count("cancelled");
    const statusWord = batch.status === "completed" ? canvasT("videoCanvas.script.batchDone", "完成") : batch.status === "cancelled" ? canvasT("videoCanvas.script.batchStopped", "已停止") : canvasT("videoCanvas.script.batchRunning", "中");
    return canvasT("videoCanvas.script.batchSummary", "{{mode}}{{status}} · 完成 {{ok}}/{{total}} / 失败 {{failed}} / 生成中 {{running}} / 等待 {{waiting}}", { mode: generationBatchModeLabel(batch), status: statusWord, ok: count("succeeded"), total: batch.items.length, failed: count("failed"), running: generating, waiting: count("waiting") }) + (stopped ? canvasT("videoCanvas.script.batchSummaryStopped", " / 已停止 {{stopped}}", { stopped }) : "");
}

function generationBatchItemLabel(item: CanvasGenerationBatchItem) {
    if (item.costUncertain) return canvasT("videoCanvas.script.costUncertain", "费用待确认");
    if (isContentModerationError(item.errorDetails)) return canvasT("videoCanvas.script.moderationFail", "审核未通过，需修改提示词");
    const labels: Record<CanvasGenerationBatchItemStatus, string> = { waiting: canvasT("videoCanvas.script.itemWaiting", "等待"), submitting: canvasT("videoCanvas.script.itemSubmitting", "提交中"), queued: canvasT("videoCanvas.script.itemQueued", "排队"), running: canvasT("videoCanvas.script.itemRunning", "生成中"), succeeded: canvasT("videoCanvas.script.itemSucceeded", "成功"), failed: canvasT("videoCanvas.script.itemFailed", "失败"), cancelled: canvasT("videoCanvas.script.itemCancelled", "已停止") };
    return labels[item.status];
}

function batchItemTone(item?: CanvasGenerationBatchItem): CanvasNodeStatus | undefined {
    if (!item) return undefined;
    if (item.status === "succeeded") return "success";
    if (item.status === "failed" || item.status === "cancelled") return "error";
    if (item.status === "waiting") return "idle";
    return "loading";
}

function CompactInput({ value, placeholder, borderColor, onChange }: { value: string; placeholder: string; borderColor: string; onChange: (value: string) => void }) {
    return <textarea className="thin-scrollbar h-full w-full resize-none overflow-y-auto overflow-x-hidden whitespace-pre-wrap break-words border-r bg-transparent px-4 py-2.5 text-xs leading-5 outline-none transition placeholder:opacity-35 focus:bg-black/[0.02] dark:focus:bg-white/[0.025]" style={{ borderColor }} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} onMouseDown={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()} />;
}

function HeaderCell({ children, borderColor, align = "left" }: { children: ReactNode; borderColor: string; align?: "left" | "center" }) {
    return <span className={`flex h-full items-center border-r px-4 ${align === "center" ? "justify-center text-center" : "justify-start"}`} style={{ borderColor }}>{children}</span>;
}

function SmallButton({ title, children, onClick }: { title: string; children: ReactNode; onClick: () => void }) {
    return <button type="button" className="grid size-7 shrink-0 place-items-center rounded opacity-65 transition hover:bg-black/5 hover:opacity-100 dark:hover:bg-white/10" title={title} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onClick(); }}>{children}</button>;
}

function RowHandle({ side, top, tone, theme, title, onPointerDown }: { side: "left" | "right"; top: number; tone?: StoryboardRow["status"]; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; title?: string; onPointerDown: (event: ReactPointerEvent) => void }) {
    const color = tone === "loading" ? theme.accent.primary : tone === "error" ? theme.accent.danger : tone === "success" ? theme.node.activeStroke : theme.node.muted;
    return (
        <button
            type="button"
            aria-label={title || `${side === "left" ? canvasT("videoCanvas.script.handleIn", "输入") : canvasT("videoCanvas.script.handleOut", "输出")}${canvasT("videoCanvas.script.handlePoint", "连接点")}`}
            title={title || (side === "left" ? canvasT("videoCanvas.script.handleInTitle", "引入参考") : canvasT("videoCanvas.script.handleOutTitle", "连接到图片、视频或生成节点"))}
            className={`canvas-connection-handle absolute z-[var(--node-z-handle)] flex -translate-y-1/2 cursor-pointer items-center justify-center rounded-full outline-none focus-visible:ring-2 ${side === "left" ? "left-0 -translate-x-1/2" : "right-0 translate-x-1/2"}`}
            style={{ top, width: "calc(32px * var(--canvas-live-inverse-scale, 1))", height: "calc(32px * var(--canvas-live-inverse-scale, 1))", "--tw-ring-color": theme.accent.primary } as CSSProperties}
            onPointerDown={onPointerDown}
        >
            <span className="block size-2.5 rounded-full border-2 shadow-sm transition-transform hover:scale-110" style={{ boxSizing: "border-box", borderColor: theme.node.panel, background: color }} />
        </button>
    );
}
