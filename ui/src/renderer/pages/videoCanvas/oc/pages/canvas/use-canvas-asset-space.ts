import { useEffect, useMemo, useState } from "react";

import { listBriefingSessions } from "@renderer/pages/videoGeneration/briefing/api";
import { listSessions } from "@renderer/pages/videoGeneration/api";
import { listGenerationTasks } from "@renderer/pages/videoCanvas/api";
import type { CanvasNodeData } from "@oc/types/canvas";

import {
    assetSpaceItemsFromBriefing,
    assetSpaceItemsFromCanvasNodes,
    assetSpaceItemsFromDrama,
    assetSpaceItemsFromGenerate,
    countAssetSpaceByDramaCategory,
    countAssetSpaceByKind,
    countAssetSpaceBySource,
    filterAssetSpaceItems,
    type AssetSpaceDramaCategory,
    type AssetSpaceItem,
    type AssetSpaceKind,
    type AssetSpaceSource,
} from "@oc/lib/canvas/canvas-asset-space";
import { loadDramaPlateItems } from "@oc/lib/canvas/canvas-asset-space-drama";

const DRAMA_SESSION_CAP = 80;
const BRIEFING_SESSION_CAP = 40;

export function useCanvasAssetSpace(nodes: CanvasNodeData[], enabled: boolean) {
    const canvasItems = useMemo(() => assetSpaceItemsFromCanvasNodes(nodes), [nodes]);
    const [remoteItems, setRemoteItems] = useState<AssetSpaceItem[]>([]);
    const [loading, setLoading] = useState(false);
    const [loadingDramaPlates, setLoadingDramaPlates] = useState(false);

    useEffect(() => {
        if (!enabled) return;
        let cancelled = false;
        setLoading(true);
        setLoadingDramaPlates(false);
        void Promise.allSettled([
            listSessions(),
            listGenerationTasks(50, 0, { standalone: true }),
            listBriefingSessions(),
        ]).then(async ([drama, generate, briefing]) => {
            if (cancelled) return;
            const dramaSessions = drama.status === "fulfilled" ? drama.value.slice(0, DRAMA_SESSION_CAP) : [];
            const items: AssetSpaceItem[] = [
                ...assetSpaceItemsFromDrama(dramaSessions),
                ...(generate.status === "fulfilled" ? assetSpaceItemsFromGenerate(generate.value.tasks) : []),
                ...(briefing.status === "fulfilled" ? assetSpaceItemsFromBriefing(briefing.value.slice(0, BRIEFING_SESSION_CAP)) : []),
            ];
            setRemoteItems(items);
            setLoading(false);
            if (!dramaSessions.length) return;
            setLoadingDramaPlates(true);
            try {
                const plates = await loadDramaPlateItems(dramaSessions);
                if (cancelled) return;
                setRemoteItems((current) => mergeDramaItems(current, plates));
            } finally {
                if (!cancelled) setLoadingDramaPlates(false);
            }
        });
        return () => {
            cancelled = true;
        };
    }, [enabled]);

    const items = useMemo(() => [...canvasItems, ...remoteItems], [canvasItems, remoteItems]);
    const counts = useMemo(() => countAssetSpaceBySource(items), [items]);

    return { items, counts, loading, loadingDramaPlates };
}

export function useFilteredAssetSpace(
    items: AssetSpaceItem[],
    source: AssetSpaceSource | "all",
    kind: AssetSpaceKind | "all",
    query: string,
    category: AssetSpaceDramaCategory | "all" = "all",
) {
    return useMemo(
        () => filterAssetSpaceItems(items, { source, kind, category, query }).toSorted((a, b) => b.updatedAt - a.updatedAt || a.title.localeCompare(b.title, "zh")),
        [category, items, kind, query, source],
    );
}

export function useAssetSpaceKindCounts(items: AssetSpaceItem[], source: AssetSpaceSource | "all", category: AssetSpaceDramaCategory | "all" = "all") {
    return useMemo(() => {
        const scoped = filterAssetSpaceItems(items, { source, category });
        return countAssetSpaceByKind(scoped);
    }, [category, items, source]);
}

export function useAssetSpaceDramaCategoryCounts(items: AssetSpaceItem[]) {
    return useMemo(() => countAssetSpaceByDramaCategory(items.filter((item) => item.source === "drama")), [items]);
}

function mergeDramaItems(current: AssetSpaceItem[], plates: AssetSpaceItem[]) {
    const seen = new Set(current.map((item) => item.preview.type === "vimax" ? `${item.preview.sessionId}:${item.preview.path}` : item.id));
    const extra = plates.filter((item) => {
        if (item.preview.type !== "vimax") return !seen.has(item.id);
        const key = `${item.preview.sessionId}:${item.preview.path}`;
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
    });
    return extra.length ? [...current, ...extra] : current;
}
