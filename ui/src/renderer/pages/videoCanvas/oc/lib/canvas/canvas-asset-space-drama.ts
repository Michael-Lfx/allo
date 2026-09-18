import { buildBackendAuthHeaders } from "@/common/adapter/httpBridge";
import { artifactFileUrl, listActionAssets, listCameos } from "@renderer/pages/videoGeneration/api";
import type { SessionSummary } from "@renderer/pages/videoGeneration/types";

import {
    assetSpaceItemsFromDramaActionAssets,
    assetSpaceItemsFromDramaCameos,
    assetSpaceItemsFromPortraitRegistry,
    assetSpaceItemsFromWorldRegistry,
    dramaRegistryJsonPaths,
    selectDramaSessionsForPlates,
    type AssetSpaceItem,
} from "@oc/lib/canvas/canvas-asset-space";

const PLATE_FETCH_CONCURRENCY = 4;

export async function loadDramaPlateItems(sessions: SessionSummary[]): Promise<AssetSpaceItem[]> {
    const selected = selectDramaSessionsForPlates(sessions);
    if (!selected.length) return [];
    const results = await mapPool(selected, PLATE_FETCH_CONCURRENCY, loadDramaSessionPlateItems);
    return results.flat();
}

export async function loadDramaSessionPlateItems(session: SessionSummary): Promise<AssetSpaceItem[]> {
    const [portrait, world, cameos, action] = await Promise.all([
        firstJson(session.id, dramaRegistryJsonPaths(session, "character_portraits_registry.json")),
        firstJson(session.id, dramaRegistryJsonPaths(session, "world_assets_registry.json")),
        listCameos(session.id).catch(() => []),
        session.workflow === "action2video" ? listActionAssets(session.id).catch(() => null) : Promise.resolve(null),
    ]);
    return [
        ...assetSpaceItemsFromPortraitRegistry(session, portrait),
        ...assetSpaceItemsFromWorldRegistry(session, world),
        ...assetSpaceItemsFromDramaCameos(session, cameos),
        ...(action ? assetSpaceItemsFromDramaActionAssets(session, action) : []),
    ];
}

async function firstJson(sessionId: string, paths: string[]): Promise<unknown | null> {
    const unique = [...new Set(paths.filter(Boolean))];
    const results = await Promise.all(unique.map((path) => readVimaxJson(sessionId, path)));
    return results.find((value) => isUsefulRegistry(value)) ?? null;
}

function isUsefulRegistry(value: unknown): boolean {
    if (!value || typeof value !== "object") return false;
    if (Array.isArray(value)) return value.length > 0;
    return Object.keys(value as Record<string, unknown>).length > 0;
}

async function readVimaxJson(sessionId: string, path: string): Promise<unknown | null> {
    try {
        const response = await fetch(artifactFileUrl(sessionId, path), {
            method: "GET",
            headers: buildBackendAuthHeaders("GET"),
            credentials: "omit",
            cache: "no-store",
        });
        if (!response.ok) return null;
        const text = (await response.text()).replace(/^\uFEFF/, "").trim();
        if (!text) return null;
        const parsed = JSON.parse(text) as unknown;
        if (!parsed || typeof parsed !== "object") return parsed;
        const rec = parsed as Record<string, unknown>;
        if (("success" in rec || "code" in rec) && rec.data && typeof rec.data === "object") return rec.data;
        return parsed;
    } catch {
        return null;
    }
}

async function mapPool<T, R>(items: T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
    const out: R[] = new Array(items.length);
    let next = 0;
    const worker = async () => {
        while (next < items.length) {
            const index = next;
            next += 1;
            out[index] = await fn(items[index]!);
        }
    };
    await Promise.all(Array.from({ length: Math.min(Math.max(limit, 1), items.length) }, () => worker()));
    return out;
}
