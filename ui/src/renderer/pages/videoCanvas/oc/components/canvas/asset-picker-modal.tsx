import { useEffect, useMemo, useState } from "react";
import { Input, Modal, Pagination, Tag } from "antd";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { WorkspaceState } from "@oc/components/layout/workspace-state";
import { AssetMediaPreview } from "@oc/components/asset-media-preview";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { cn } from "@oc/lib/utils";
import { useAssetStore, type Asset } from "@oc/stores/use-asset-store";

type InsertableAsset = Extract<Asset, { kind: "text" | "image" | "video" | "audio" }>;

export type InsertAssetPayload =
    | { kind: "text"; content: string; title: string; assetId?: string }
    | { kind: "image"; dataUrl: string; title: string; storageKey?: string; assetId?: string }
    | { kind: "video"; url: string; title: string; storageKey?: string; width?: number; height?: number; durationMs?: number; bytes?: number; mimeType?: string; assetId?: string }
    | { kind: "audio"; url: string; title: string; storageKey?: string; durationMs?: number; bytes?: number; mimeType?: string; assetId?: string }
    | { kind: "character"; title: string; assetId: string; versionId: string; prompt: string; aliases: string[]; definition: Record<string, unknown>; coverUrl?: string; visualStatus: string; voiceStatus: string; voiceName?: string; voiceProfile?: { name: string; provider: string; language: string; timbre: string }; voiceInstructions?: string };

type Props = {
    open: boolean;
    defaultTab?: string;
    onInsert: (payload: InsertAssetPayload) => void;
    onClose: () => void;
};

export function AssetPickerModal({ open, onInsert, onClose }: Props) {
    useTranslation();
    return (
        <Modal title={canvasT("videoCanvas.asset.pickerTitle", "选择素材")} open={open} onCancel={onClose} footer={null} width={860} destroyOnHidden styles={{ body: { padding: "0 24px 24px", minHeight: 480 } }}>
            <MyAssetsTab onInsert={onInsert} />
        </Modal>
    );
}

const PAGE_SIZE = 8;

function kindOptions() {
    return [
        { label: canvasT("videoCanvas.asset.kindAll", "全部"), value: "all" },
        { label: canvasT("videoCanvas.asset.kindText", "文本"), value: "text" },
        { label: canvasT("videoCanvas.asset.kindImage", "图片"), value: "image" },
        { label: canvasT("videoCanvas.asset.kindVideo", "视频"), value: "video" },
        { label: canvasT("videoCanvas.asset.kindAudio", "音频"), value: "audio" },
    ];
}

function kindLabel(kind: InsertableAsset["kind"]) {
    if (kind === "image") return canvasT("videoCanvas.asset.kindImage", "图片");
    if (kind === "video") return canvasT("videoCanvas.asset.kindVideo", "视频");
    if (kind === "audio") return canvasT("videoCanvas.asset.kindAudio", "音频");
    return canvasT("videoCanvas.asset.kindText", "文本");
}

function PickerCard({ asset, onClick }: { asset: InsertableAsset; onClick: () => void }) {
    useTranslation();
    const { title, kind } = asset;
    return (
        <button
            type="button"
            className="group relative cursor-pointer overflow-hidden rounded-lg border border-stone-200 bg-white text-left transition hover:border-stone-400 hover:shadow-md dark:border-stone-700 dark:bg-stone-900 dark:hover:border-stone-500"
            onClick={onClick}
        >
            <AssetMediaPreview asset={asset} alt={title} className="aspect-[4/3] w-full bg-black object-cover" fallback={<div className="flex aspect-[4/3] items-center justify-center bg-stone-100 p-3 text-center text-xs leading-5 text-stone-500 dark:bg-stone-800 dark:text-stone-400">{title}</div>} />
            <div className="p-2.5">
                <div className="flex items-center justify-between gap-2">
                    <span className="line-clamp-1 text-xs font-medium text-stone-800 dark:text-stone-200">{title}</span>
                    <Tag className="m-0 shrink-0 text-[var(--fs-tiny)]">{kindLabel(kind)}</Tag>
                </div>
            </div>
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-stone-950/0 text-sm font-medium text-white opacity-0 transition group-hover:bg-stone-950/55 group-hover:opacity-100">{canvasT("videoCanvas.asset.insert", "插入")}</div>
        </button>
    );
}

function MyAssetsTab({ onInsert }: { onInsert: (payload: InsertAssetPayload) => void }) {
    useTranslation();
    const assets = useAssetStore((state) => state.assets);
    const [keyword, setKeyword] = useState("");
    const [kindFilter, setKindFilter] = useState("all");
    const [page, setPage] = useState(1);

    const filtered = useMemo(() => {
        const query = keyword.trim().toLowerCase();
        return assets
            .filter((asset): asset is InsertableAsset => asset.kind === "text" || asset.kind === "image" || asset.kind === "video" || asset.kind === "audio")
            .filter((a) => kindFilter === "all" || a.kind === kindFilter)
            .filter((a) => !query || [a.title, ...(a.tags || [])].join(" ").toLowerCase().includes(query));
    }, [assets, keyword, kindFilter]);

    const visible = useMemo(() => filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE), [filtered, page]);

    useEffect(() => {
        const maxPage = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
        setPage((v) => Math.min(v, maxPage));
    }, [filtered.length]);

    const handleInsert = (asset: InsertableAsset) => {
        if (asset.kind === "text") {
            onInsert({ kind: "text", content: asset.data.content, title: asset.title, assetId: asset.id });
        } else if (asset.kind === "audio") {
            onInsert({ kind: "audio", url: asset.data.url, storageKey: asset.data.storageKey, title: asset.title, durationMs: asset.data.durationMs, bytes: asset.data.bytes, mimeType: asset.data.mimeType, assetId: asset.id });
        } else if (asset.kind === "video") {
            onInsert({ kind: "video", url: asset.data.url, storageKey: asset.data.storageKey, title: asset.title, width: asset.data.width, height: asset.data.height, durationMs: asset.data.durationMs, bytes: asset.data.bytes, mimeType: asset.data.mimeType, assetId: asset.id });
        } else if (asset.kind === "image") {
            onInsert({ kind: "image", dataUrl: asset.data.dataUrl, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id });
        }
    };

    return (
        <div className="space-y-4">
            <div className="flex flex-wrap items-center gap-3">
                <Input
                    className="w-56"
                    size="small"
                    prefix={<Search className="size-3.5 text-stone-400" />}
                    placeholder={canvasT("videoCanvas.asset.searchAssets", "搜索素材")}
                    value={keyword}
                    allowClear
                    onChange={(e) => {
                        setPage(1);
                        setKeyword(e.target.value);
                    }}
                />
                <div className="flex gap-1.5">
                    {kindOptions().map((opt) => (
                        <Tag.CheckableTag
                            key={opt.value}
                            checked={kindFilter === opt.value}
                            className={cn("prompt-filter-tag", kindFilter === opt.value && "is-active")}
                            onChange={() => {
                                setPage(1);
                                setKindFilter(opt.value);
                            }}
                        >
                            {opt.label}
                        </Tag.CheckableTag>
                    ))}
                </div>
            </div>

            {visible.length ? (
                <div className="grid grid-cols-4 gap-3">
                    {visible.map((asset) => (
                        <PickerCard key={asset.id} asset={asset} onClick={() => handleInsert(asset)} />
                    ))}
                </div>
            ) : (
                <WorkspaceState
                    icon="assets"
                    compact
                    title={canvasT("videoCanvas.asset.emptyTitle", "没有可用素材")}
                    description={keyword || kindFilter !== "all" ? canvasT("videoCanvas.asset.emptyFiltered", "调整关键词或素材类型后再试。") : canvasT("videoCanvas.asset.emptyHint", "先在素材库中添加图片、视频、音频或文本。")}
                />
            )}

            {filtered.length > PAGE_SIZE && (
                <div className="flex justify-center">
                    <Pagination size="small" current={page} pageSize={PAGE_SIZE} total={filtered.length} onChange={setPage} showSizeChanger={false} />
                </div>
            )}
        </div>
    );
}
