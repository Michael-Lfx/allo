import { create } from "zustand";
import { persist, type PersistStorage, type StorageValue } from "zustand/middleware";

import { nanoid } from "nanoid";
import { localForageStorage } from "@oc/lib/localforage-storage";
import { cleanupUnusedImages, resolveImageUrl, uploadImage } from "@oc/services/image-storage";
import { cleanupUnusedMedia, resolveMediaUrl, uploadMediaFile } from "@oc/services/file-storage";

export type AssetKind = "text" | "image" | "video" | "audio" | "model" | "entity";
export type AssetCategory = "character" | "environment" | "wardrobe" | "prop" | "weapon" | "style" | "other";
export type AssetStatus = "draft" | "review" | "confirmed" | "archived";
export type TextAsset = AssetBase<"text"> & { data: { content: string } };
export type ImageAsset = AssetBase<"image"> & { data: { dataUrl: string; storageKey?: string; width: number; height: number; bytes: number; mimeType: string } };
export type VideoAsset = AssetBase<"video"> & { data: { url: string; storageKey?: string; width: number; height: number; durationMs?: number; bytes: number; mimeType: string } };
export type AudioAsset = AssetBase<"audio"> & { data: { url: string; storageKey?: string; durationMs?: number; bytes: number; mimeType: string } };
export type ModelAsset = AssetBase<"model"> & { data: { url: string; storageKey?: string; bytes: number; mimeType: string; fileName: string } };
export type EntityAsset = AssetBase<"entity"> & { data: { definition: Record<string, unknown> } };
export type Asset = TextAsset | ImageAsset | VideoAsset | AudioAsset | ModelAsset | EntityAsset;
export type NewAsset =
    | Omit<TextAsset, "id" | "createdAt" | "updatedAt">
    | Omit<ImageAsset, "id" | "createdAt" | "updatedAt">
    | Omit<VideoAsset, "id" | "createdAt" | "updatedAt">
    | Omit<AudioAsset, "id" | "createdAt" | "updatedAt">
    | Omit<ModelAsset, "id" | "createdAt" | "updatedAt">
    | Omit<EntityAsset, "id" | "createdAt" | "updatedAt">;

type AssetBase<T extends AssetKind> = {
    id: string;
    kind: T;
    title: string;
    coverUrl: string;
    tags: string[];
    category?: AssetCategory;
    status?: AssetStatus;
    primaryVersionId?: string;
    source?: string;
    note?: string;
    createdAt: string;
    updatedAt: string;
    metadata?: Record<string, unknown>;
};

type AssetStore = {
    hydrated: boolean;
    assets: Asset[];
    addAsset: (asset: NewAsset) => string;
    updateAsset: (id: string, patch: Partial<Omit<Asset, "id" | "createdAt">>) => void;
    removeAsset: (id: string) => void;
    replaceAssets: (assets: Asset[]) => void;
    cleanupImages: (extra?: unknown) => void;
};
export const ASSET_STORE_KEY = "infinite-canvas:asset_store";

// 运行期 blob: URL 只在会话内有效：持久化前把尚无 storageKey 的 blob 条目上传落盘并换为
// storageKey，反解失败则跳过该条（getItem 侧已有按 storageKey 反解路径，下次写入会再试）。
// 写入带防抖——拖拽/连续编辑只落最新状态；队列串行化，新条目不会被旧写覆盖丢弃。
const ASSET_PERSIST_DEBOUNCE_MS = 500;

let assetPersistTimer: ReturnType<typeof setTimeout> | undefined;
let assetPersistPending: { name: string; value: StorageValue<AssetStore> } | null = null;

async function normalizeAssetsForPersist(assets: Asset[]): Promise<Asset[]> {
    return Promise.all(
        assets.map(async (asset): Promise<Asset> => {
            try {
                if (asset.kind === "image") {
                    if (asset.data.storageKey || !asset.data.dataUrl.startsWith("blob:")) return asset;
                    const uploaded = await uploadImage(asset.data.dataUrl);
                    return {
                        ...asset,
                        coverUrl: asset.coverUrl.startsWith("blob:") ? uploaded.url : asset.coverUrl,
                        data: { ...asset.data, dataUrl: uploaded.url, storageKey: uploaded.storageKey, bytes: uploaded.bytes, mimeType: uploaded.mimeType },
                    };
                }
                if (asset.kind === "video" && !asset.data.storageKey && asset.data.url.startsWith("blob:")) {
                    const uploaded = await uploadMediaFile(asset.data.url, "video");
                    return {
                        ...asset,
                        coverUrl: asset.coverUrl.startsWith("blob:") ? uploaded.url : asset.coverUrl,
                        data: {
                            ...asset.data,
                            url: uploaded.url,
                            storageKey: uploaded.storageKey,
                            bytes: uploaded.bytes,
                            mimeType: uploaded.mimeType,
                            ...(uploaded.width != null ? { width: uploaded.width } : null),
                            ...(uploaded.height != null ? { height: uploaded.height } : null),
                            ...(uploaded.durationMs != null ? { durationMs: uploaded.durationMs } : null),
                        },
                    };
                }
                if (asset.kind === "audio" && !asset.data.storageKey && asset.data.url.startsWith("blob:")) {
                    const uploaded = await uploadMediaFile(asset.data.url, "audio");
                    return {
                        ...asset,
                        coverUrl: asset.coverUrl.startsWith("blob:") ? uploaded.url : asset.coverUrl,
                        data: {
                            ...asset.data,
                            url: uploaded.url,
                            storageKey: uploaded.storageKey,
                            bytes: uploaded.bytes,
                            mimeType: uploaded.mimeType,
                            ...(uploaded.durationMs != null ? { durationMs: uploaded.durationMs } : null),
                        },
                    };
                }
                if (asset.kind === "model" && !asset.data.storageKey && asset.data.url.startsWith("blob:")) {
                    const uploaded = await uploadMediaFile(asset.data.url, "model");
                    return {
                        ...asset,
                        coverUrl: asset.coverUrl.startsWith("blob:") ? uploaded.url : asset.coverUrl,
                        data: { ...asset.data, url: uploaded.url, storageKey: uploaded.storageKey, bytes: uploaded.bytes, mimeType: uploaded.mimeType },
                    };
                }
                return asset;
            } catch {
                // 上传失败（如 blob 已失效）时保留原条目原样写入，等待下次持久化再试。
                return asset;
            }
        }),
    );
}

async function drainAssetPersist() {
    for (;;) {
        const pending = assetPersistPending;
        if (!pending) return;
        assetPersistPending = null;
        try {
            const assets = await normalizeAssetsForPersist(pending.value.state.assets);
            await localForageStorage.setItem(pending.name, JSON.stringify({ ...pending.value, state: { ...pending.value.state, assets } }));
        } catch {
            // 单次写入失败不阻塞队列中的后续条目。
        }
    }
}

const assetStorage: PersistStorage<AssetStore> = {
    getItem: async (name) => {
        const value = await localForageStorage.getItem(name);
        if (!value) return null;
        const parsed = JSON.parse(value) as StorageValue<AssetStore>;
        parsed.state.assets = await Promise.all(
            parsed.state.assets.map(async (asset) => {
                // 视频和音频的数据结构不同，分别缩窄以保持 Asset 判别联合关系。
                if (asset.kind === "video" && asset.data.storageKey) return { ...asset, data: { ...asset.data, url: await resolveMediaUrl(asset.data.storageKey, asset.data.url) } };
                if (asset.kind === "audio" && asset.data.storageKey) return { ...asset, data: { ...asset.data, url: await resolveMediaUrl(asset.data.storageKey, asset.data.url) } };
                if (asset.kind === "model" && asset.data.storageKey) return { ...asset, data: { ...asset.data, url: await resolveMediaUrl(asset.data.storageKey, asset.data.url) } };
                if (asset.kind !== "image") return asset;
                if (asset.data.storageKey)
                    return {
                        ...asset,
                        coverUrl: asset.coverUrl.startsWith("blob:") ? await resolveImageUrl(asset.data.storageKey, asset.coverUrl) : asset.coverUrl,
                        data: { ...asset.data, dataUrl: await resolveImageUrl(asset.data.storageKey, asset.data.dataUrl) },
                    };
                if (!asset.data.dataUrl.startsWith("data:image/")) return asset;
                const image = await uploadImage(asset.data.dataUrl);
                return { ...asset, coverUrl: asset.coverUrl.startsWith("data:image/") ? image.url : asset.coverUrl, data: { ...asset.data, dataUrl: image.url, storageKey: image.storageKey, bytes: image.bytes, mimeType: image.mimeType } };
            }),
        );
        return parsed;
    },
    setItem: (name, value) => {
        assetPersistPending = { name, value };
        if (assetPersistTimer !== null)  clearTimeout(assetPersistTimer);
        assetPersistTimer = setTimeout(() => {
            assetPersistTimer = undefined;
            void drainAssetPersist();
        }, ASSET_PERSIST_DEBOUNCE_MS);
        return Promise.resolve();
    },
    removeItem: (name) => localForageStorage.removeItem(name),
};

export const useAssetStore = create<AssetStore>()(
    persist(
        (set, get) => ({
            hydrated: false,
            assets: [],
            addAsset: (asset) => {
                const now = new Date().toISOString();
                const id = nanoid();
                set((state) => ({ assets: [{ ...asset, id, createdAt: now, updatedAt: now } as Asset, ...state.assets] }));
                return id;
            },
            updateAsset: (id, patch) =>
                set((state) => ({
                    assets: state.assets.map((asset) => (asset.id === id ? ({ ...asset, ...patch, updatedAt: new Date().toISOString() } as Asset) : asset)),
                })),
            removeAsset: (id) =>
                set((state) => {
                    const assets = state.assets.filter((asset) => asset.id !== id);
                    get().cleanupImages({ assets });
                    return { assets };
                }),
            replaceAssets: (assets) => set({ assets }),
            cleanupImages: (extra) => {
                window.setTimeout(async () => {
                    const { useCanvasStore } = await import("@oc/stores/canvas/use-canvas-store");
                    await cleanupUnusedImages({ assets: get().assets, projects: useCanvasStore.getState().projects, extra });
                    await cleanupUnusedMedia({ assets: get().assets, projects: useCanvasStore.getState().projects, extra });
                }, 0);
            },
        }),
        {
            name: ASSET_STORE_KEY,
            storage: assetStorage,
            partialize: (state) => ({ assets: state.assets }) as StorageValue<AssetStore>["state"],
            onRehydrateStorage: () => () => {
                useAssetStore.setState({ hydrated: true });
            },
        },
    ),
);
