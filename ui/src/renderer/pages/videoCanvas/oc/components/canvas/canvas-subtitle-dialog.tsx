import { useEffect, useRef, useState } from "react";
import { App, Button, Input, Modal } from "antd";
import { FileDown, FileUp, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { parseSrt, serializeSrtEntries } from "@oc/lib/timeline/srt-parser";
import { createDefaultSubtitleStyle, type SrtEntry } from "@oc/types/timeline";
import type { CanvasNodeData, CanvasNodeMetadata } from "@oc/types/canvas";

type CanvasSubtitleDialogProps = {
    node: CanvasNodeData;
    open: boolean;
    onClose: () => void;
    onSave: (nodeId: string, patch: Partial<CanvasNodeMetadata>) => void;
};

/** MVP：SRT 导入/导出与条目编辑，写回节点 metadata（经现有 PUT /doc 持久化）。 */
export function CanvasSubtitleDialog({ node, open, onClose, onSave }: CanvasSubtitleDialogProps) {
    useTranslation();
    const { message } = App.useApp();
    const [entries, setEntries] = useState<SrtEntry[]>([]);
    const fileInputRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        if (!open) return;
        setEntries(node.metadata?.subtitleEntries || []);
    }, [open, node]);

    const save = () => {
        onSave(node.id, {
            subtitleEntries: entries,
            subtitleStyle: node.metadata?.subtitleStyle || createDefaultSubtitleStyle(),
            subtitleUpdatedAt: new Date().toISOString(),
        });
        message.success(entries.length
            ? canvasT("videoCanvas.subtitle.saved", "已保存 {{count}} 条字幕", { count: entries.length })
            : canvasT("videoCanvas.subtitle.cleared", "已清空字幕"));
        onClose();
    };

    const importSrt = async (file: File) => {
        try {
            const text = await file.text();
            const parsed = parseSrt(text);
            if (!parsed.length) {
                message.warning(canvasT("videoCanvas.subtitle.parseEmpty", "未解析到有效字幕条目"));
                return;
            }
            setEntries(parsed);
            message.success(canvasT("videoCanvas.subtitle.saved", "已保存 {{count}} 条字幕", { count: parsed.length }));
        } catch (error) {
            message.error(error instanceof Error ? error.message : canvasT("videoCanvas.subtitle.importFailed", "导入失败"));
        }
    };

    const exportSrt = () => {
        const blob = new Blob([serializeSrtEntries(entries)], { type: "text/plain;charset=utf-8" });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = `${node.title || "subtitles"}.srt`;
        anchor.click();
        URL.revokeObjectURL(url);
    };

    return (
        <Modal
            title={canvasT("videoCanvas.subtitle.title", "字幕 · {{name}}", { name: node.title || canvasT("videoCanvas.subtitle.videoFallback", "视频") })}
            open={open}
            onCancel={onClose}
            width="min(720px, 92vw)"
            centered
            footer={[
                <Button key="cancel" onClick={onClose}>{canvasT("videoCanvas.subtitle.cancel", "取消")}</Button>,
                <Button key="save" type="primary" onClick={save}>{canvasT("videoCanvas.subtitle.save", "保存到节点")}</Button>,
            ]}
        >
            <div className="mb-3 flex flex-wrap items-center gap-2">
                <Button icon={<FileUp className="size-3.5" />} onClick={() => fileInputRef.current?.click()}>{canvasT("videoCanvas.subtitle.import", "导入 SRT")}</Button>
                <Button icon={<FileDown className="size-3.5" />} disabled={!entries.length} onClick={exportSrt}>{canvasT("videoCanvas.subtitle.export", "导出 SRT")}</Button>
                <Button
                    icon={<Plus className="size-3.5" />}
                    onClick={() => setEntries((current) => [...current, { index: current.length + 1, startMs: current.at(-1)?.endMs || 0, endMs: (current.at(-1)?.endMs || 0) + 2000, text: "" }])}
                >
                    {canvasT("videoCanvas.subtitle.add", "添加条目")}
                </Button>
                <input ref={fileInputRef} type="file" accept=".srt,text/plain" className="hidden" onChange={(event) => { const file = event.target.files?.[0]; if (file) void importSrt(file); event.target.value = ""; }} />
            </div>
            <div className="thin-scrollbar max-h-[50vh] space-y-2 overflow-y-auto">
                {entries.length ? entries.map((entry, index) => (
                    <div key={`${entry.index}-${index}`} className="grid grid-cols-[72px_72px_minmax(0,1fr)_32px] items-center gap-2">
                        <Input
                            size="small"
                            value={formatMs(entry.startMs)}
                            onChange={(event) => setEntries((current) => current.map((item, i) => (i === index ? { ...item, startMs: parseTimestamp(event.target.value, item.startMs) } : item)))}
                            aria-label="开始时间"
                        />
                        <Input
                            size="small"
                            value={formatMs(entry.endMs)}
                            onChange={(event) => setEntries((current) => current.map((item, i) => (i === index ? { ...item, endMs: parseTimestamp(event.target.value, item.endMs) } : item)))}
                            aria-label="结束时间"
                        />
                        <Input
                            size="small"
                            value={entry.text}
                            onChange={(event) => setEntries((current) => current.map((item, i) => (i === index ? { ...item, text: event.target.value } : item)))}
                            placeholder={canvasT("videoCanvas.subtitle.placeholder", "字幕文本")}
                        />
                        <Button type="text" danger icon={<Trash2 className="size-3.5" />} onClick={() => setEntries((current) => current.filter((_, i) => i !== index).map((item, i) => ({ ...item, index: i + 1 })))} />
                    </div>
                )) : <div className="rounded-md border border-dashed px-3 py-8 text-center text-sm opacity-60">{canvasT("videoCanvas.subtitle.empty", "尚无字幕，可导入 SRT 或手动添加")}</div>}
            </div>
        </Modal>
    );
}

function formatMs(ms: number) {
    const safe = Math.max(0, Math.round(ms));
    const minutes = Math.floor(safe / 60_000);
    const seconds = Math.floor((safe % 60_000) / 1000);
    const millis = safe % 1000;
    return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

function parseTimestamp(value: string, fallback: number) {
    const match = value.trim().match(/^(\d{1,2}):(\d{2})(?:[.,](\d{1,3}))?$/);
    if (!match) return fallback;
    const minutes = Number(match[1]);
    const seconds = Number(match[2]);
    const millis = Number((match[3] || "0").padEnd(3, "0"));
    if ([minutes, seconds, millis].some((n) => Number.isNaN(n))) return fallback;
    return minutes * 60_000 + seconds * 1000 + millis;
}
