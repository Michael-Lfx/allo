import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Button, Checkbox, Input, InputNumber, Modal, Segmented, Select, Table } from "antd";
import type { ColumnsType } from "antd/es/table";
import { ChevronDown, ChevronUp, Copy, Film, Image as ImageIcon, Plus, Trash2 } from "lucide-react";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";

import type { CanvasNodeData, StoryboardColumn, StoryboardRow, StoryboardVideoInputMode } from "@oc/types/canvas";

const EMPTY_STORYBOARD_ROWS: StoryboardRow[] = [];

type ColumnOption = { label: string; value: StoryboardColumn };

function buildColumnOptions(): ColumnOption[] {
    return [
        { label: canvasT("videoCanvas.script.colIndex", "序号"), value: "shotNumber" },
        { label: canvasT("videoCanvas.script.colDuration", "时长"), value: "durationSeconds" },
        { label: canvasT("videoCanvas.script.colVisual", "画面描述"), value: "plotDescription" },
        { label: canvasT("videoCanvas.script.colDialogue", "台词/旁白"), value: "dialogue" },
        { label: canvasT("videoCanvas.scriptEditor.colNarrativeIntent", "镜头意图"), value: "narrativeIntent" },
        { label: canvasT("videoCanvas.scriptEditor.colViewerPov", "观众视点"), value: "viewerPOV" },
        { label: canvasT("videoCanvas.scriptEditor.colBlocking", "表演调度"), value: "performanceBlocking" },
        { label: canvasT("videoCanvas.script.colShotSize", "景别"), value: "shotSize" },
        { label: canvasT("videoCanvas.scriptEditor.colEmotion", "情绪"), value: "emotion" },
        { label: canvasT("videoCanvas.scriptEditor.colLighting", "光影氛围"), value: "lightingAndAtmosphere" },
        { label: canvasT("videoCanvas.scriptEditor.colSfx", "音效"), value: "audioEffects" },
        { label: canvasT("videoCanvas.scriptEditor.colCamera", "镜头设计"), value: "camera" },
        { label: canvasT("videoCanvas.scriptEditor.colMotion", "运镜"), value: "motion" },
        { label: canvasT("videoCanvas.scriptEditor.colTimeBeats", "时间节拍"), value: "timeBeats" },
        { label: canvasT("videoCanvas.scriptEditor.colImagePrompt", "图片提示词"), value: "imageGenerationPrompt" },
        { label: canvasT("videoCanvas.scriptEditor.colVideoPrompt", "视频提示词"), value: "videoMotionPrompt" },
        { label: canvasT("videoCanvas.scriptEditor.colContinuity", "连续性出口"), value: "continuityOut" },
        { label: canvasT("videoCanvas.scriptEditor.colNegative", "负面要求"), value: "negativePrompt" },
    ];
}

export function CanvasScriptEditor({ node, open, onClose, onUpdateRows, onVisibleColumnsChange, onGenerateImages, onGenerateVideos, onVideoInputModeChange }: {
    node: CanvasNodeData | null;
    open: boolean;
    onClose: () => void;
    onUpdateRows: (rows: StoryboardRow[]) => void;
    onVisibleColumnsChange: (columns: StoryboardColumn[]) => void;
    onGenerateImages: (rowIds: string[]) => void;
    onGenerateVideos: (rowIds: string[]) => void;
    onVideoInputModeChange: (mode: StoryboardVideoInputMode) => void;
}) {
    const [query, setQuery] = useState("");
    const [selectedIds, setSelectedIds] = useState<string[]>([]);
    const rows = node?.metadata?.storyboard?.rows || EMPTY_STORYBOARD_ROWS;
    const visibleColumns = node?.metadata?.storyboard?.visibleColumns || ["shotNumber", "durationSeconds", "plotDescription", "dialogue"];
    const videoInputMode = node?.metadata?.storyboardVideoInputMode || "direct";
    const columnOptions = buildColumnOptions();
    const filteredRows = useMemo(() => {
        const keyword = query.trim().toLowerCase();
        return keyword ? rows.filter((row) => [row.plotDescription, row.dialogue, row.camera, row.motion, row.timeBeats, row.imageGenerationPrompt, row.videoMotionPrompt, row.negativePrompt].some((value) => String(value || "").toLowerCase().includes(keyword))) : rows;
    }, [query, rows]);
    useEffect(() => {
        setSelectedIds((current) => {
            const next = current.filter((id) => rows.some((row) => row.id === id));
            return next.length === current.length && next.every((id, index) => id === current[index]) ? current : next;
        });
    }, [rows]);
    const updateRow = (rowId: string, patch: Partial<StoryboardRow>) => onUpdateRows(rows.map((row) => row.id === rowId ? { ...row, ...patch } : row));
    const moveRow = (rowId: string, direction: -1 | 1) => {
        const index = rows.findIndex((row) => row.id === rowId);
        const nextIndex = index + direction;
        if (index < 0 || nextIndex < 0 || nextIndex >= rows.length) return;
        const next = [...rows];
        [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
        onUpdateRows(next.map((row, rowIndex) => ({ ...row, shotNumber: rowIndex + 1 })));
    };
    const duplicateRow = (row: StoryboardRow) => {
        const index = rows.findIndex((item) => item.id === row.id);
        const next = [...rows];
        next.splice(index + 1, 0, { ...row, id: `shot-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`, imageNodeId: undefined, videoNodeId: undefined, status: "idle" });
        onUpdateRows(next.map((item, rowIndex) => ({ ...item, shotNumber: rowIndex + 1 })));
    };
    const removeRow = (rowId: string) => onUpdateRows(rows.filter((row) => row.id !== rowId).map((row, index) => ({ ...row, shotNumber: index + 1 })));

    const columns: ColumnsType<StoryboardRow> = columnOptions.filter((option) => visibleColumns.includes(option.value)).map((option) => ({
        title: option.label,
        dataIndex: option.value,
        key: option.value,
        width: option.value === "shotNumber" ? 72 : option.value === "durationSeconds" ? 100 : option.value === "plotDescription" || option.value === "dialogue" || option.value === "timeBeats" || option.value.endsWith("Prompt") ? 260 : 170,
        fixed: option.value === "shotNumber" ? "left" as const : undefined,
        render: (_: unknown, row: StoryboardRow) => option.value === "shotNumber" ? <span className="font-semibold">{row.shotNumber}</span> : option.value === "durationSeconds" ? <InputNumber min={1} max={60} value={row.durationSeconds} addonAfter="s" onChange={(value) => updateRow(row.id, { durationSeconds: Number(value) || 1 })} /> : option.value === "shotSize" ? <Select className="w-full" value={row.shotSize || undefined} placeholder={canvasT("videoCanvas.scriptEditor.shotSizePlaceholder", "选择景别")} options={[canvasT("videoCanvas.scriptEditor.shotSizeClose", "特写"), canvasT("videoCanvas.scriptEditor.shotSizeNear", "近景"), canvasT("videoCanvas.scriptEditor.shotSizeMedium", "中景"), canvasT("videoCanvas.scriptEditor.shotSizeFull", "全景"), canvasT("videoCanvas.scriptEditor.shotSizeFar", "远景")].map((value) => ({ value, label: value }))} onChange={(shotSize) => updateRow(row.id, { shotSize })} /> : <Input.TextArea autoSize={{ minRows: 1, maxRows: 4 }} value={String(row[option.value] || "")} placeholder={canvasT("videoCanvas.scriptEditor.fillTemplate", "填写{{label}}", { label: option.label })} onChange={(event) => updateRow(row.id, { [option.value]: event.target.value } as Partial<StoryboardRow>)} />,
    }));
    columns.push({
        title: canvasT("videoCanvas.scriptEditor.actions", "操作"), key: "actions", dataIndex: "shotNumber", width: 150, fixed: "right" as const,
        render: (_: unknown, row: StoryboardRow) => <div className="flex gap-1"><SmallButton title={canvasT("videoCanvas.scriptEditor.moveUp", "上移")} onClick={() => moveRow(row.id, -1)}><ChevronUp className="size-3.5" /></SmallButton><SmallButton title={canvasT("videoCanvas.scriptEditor.moveDown", "下移")} onClick={() => moveRow(row.id, 1)}><ChevronDown className="size-3.5" /></SmallButton><SmallButton title={canvasT("videoCanvas.scriptEditor.duplicate", "复制")} onClick={() => duplicateRow(row)}><Copy className="size-3.5" /></SmallButton><SmallButton title={canvasT("videoCanvas.scriptEditor.remove", "删除")} onClick={() => removeRow(row.id)}><Trash2 className="size-3.5" /></SmallButton></div>,
    });

    return (
        <Modal title={node?.title || canvasT("videoCanvas.scriptEditor.modalTitle", "分镜脚本")} open={open} onCancel={onClose} footer={null} width="min(1480px, calc(100vw - 40px))" centered destroyOnHidden>
            <div className="mb-3 flex flex-wrap items-center gap-2">
                <Input.Search className="w-72" allowClear placeholder={canvasT("videoCanvas.scriptEditor.searchPlaceholder", "筛选画面、台词或提示词")} value={query} onChange={(event) => setQuery(event.target.value)} />
                <Checkbox.Group className="script-column-picker" options={columnOptions} value={visibleColumns} onChange={(values) => onVisibleColumnsChange(values as StoryboardColumn[])} />
                <span className="min-w-0 flex-1" />
                <Button icon={<Plus className="size-4" />} onClick={() => onUpdateRows([...rows, editorRow(rows.length + 1)])}>{canvasT("videoCanvas.scriptEditor.addShot", "新增镜头")}</Button>
                <Button icon={<ImageIcon className="size-4" />} disabled={!selectedIds.length} onClick={() => onGenerateImages(selectedIds)}>{videoInputMode === "keyframe" ? canvasT("videoCanvas.scriptEditor.genKeyframes", "生成首帧") : canvasT("videoCanvas.scriptEditor.genStoryboards", "生成分镜图")}</Button>
                <Segmented<StoryboardVideoInputMode> value={videoInputMode} options={[{ value: "direct", label: canvasT("videoCanvas.script.directGen", "直接生成") }, { value: "keyframe", label: canvasT("videoCanvas.script.keyframeFirst", "先做首帧") }]} onChange={onVideoInputModeChange} />
                <Button type="primary" icon={<Film className="size-4" />} disabled={!selectedIds.length} onClick={() => onGenerateVideos(selectedIds)}>{videoInputMode === "keyframe" ? canvasT("videoCanvas.scriptEditor.confirmKeyframeGen", "确认首帧并生成") : canvasT("videoCanvas.scriptEditor.genVideos", "生成视频")}</Button>
            </div>
            <Table<StoryboardRow> rowKey="id" size="small" bordered sticky pagination={false} scroll={{ x: Math.max(900, columns.length * 180), y: "calc(78vh - 170px)" }} dataSource={filteredRows} columns={columns} rowSelection={{ selectedRowKeys: selectedIds, onChange: (keys) => setSelectedIds(keys.map(String)) }} />
        </Modal>
    );
}

function SmallButton({ title, children, onClick }: { title: string; children: ReactNode; onClick: () => void }) {
    return <button type="button" className="grid size-7 shrink-0 place-items-center rounded opacity-65 transition hover:bg-black/5 hover:opacity-100 dark:hover:bg-white/10" title={title} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onClick(); }}>{children}</button>;
}

function editorRow(shotNumber: number): StoryboardRow {
    return { id: `shot-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`, shotNumber, durationSeconds: 6, plotDescription: "", dialogue: "", characters: [], narrativeIntent: "", viewerPOV: "", performanceBlocking: "", shotSize: "", emotion: "", lightingAndAtmosphere: "", audioEffects: "", camera: "", motion: "", timeBeats: "", imageGenerationPrompt: "", videoMotionPrompt: "", mustHave: [], optionalDetails: [], continuityOut: "", negativePrompt: "", referenceNodeIds: [], status: "idle" };
}
