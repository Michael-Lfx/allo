import { useEffect, useState } from "react";

import { buildBackendAuthHeaders } from "@/common/adapter/httpBridge";
import { briefingArtifactUrl } from "@renderer/pages/videoGeneration/briefing/api";
import { acquireCachedArtifactMediaUrl, releaseCachedArtifactMediaUrl } from "@renderer/pages/videoGeneration/api";
import { useArtifactMediaUrl } from "@renderer/pages/videoGeneration/useArtifactMediaUrl";
import type { AssetSpaceKind } from "@oc/lib/canvas/canvas-asset-space";

const briefingUrls = new Map<string, { url: string; refs: number }>();
const briefingInflight = new Map<string, Promise<string>>();

export function useVimaxAssetPreview(sessionId: string | undefined, path: string | null | undefined) {
    return useArtifactMediaUrl(sessionId, path);
}

export function useBriefingAssetPreview(sessionId: string | undefined, path: string | null | undefined) {
    const [url, setUrl] = useState<string | null>(null);
    useEffect(() => {
        if (!sessionId || !path) {
            setUrl(null);
            return;
        }
        let cancelled = false;
        let loaned = false;
        void acquireBriefingMediaUrl(sessionId, path)
            .then((next) => {
                if (cancelled) {
                    releaseBriefingMediaUrl(sessionId, path);
                    return;
                }
                loaned = true;
                setUrl(next);
            })
            .catch(() => {
                if (!cancelled) setUrl(null);
            });
        return () => {
            cancelled = true;
            if (loaned) releaseBriefingMediaUrl(sessionId, path);
        };
    }, [path, sessionId]);
    return url;
}

export async function acquireBriefingMediaUrl(sessionId: string, path: string): Promise<string> {
    const key = `${sessionId}:${path}`;
    const hit = briefingUrls.get(key);
    if (hit) {
        hit.refs += 1;
        return hit.url;
    }
    const pending = briefingInflight.get(key);
    if (pending) {
        const url = await pending;
        const entry = briefingUrls.get(key);
        if (entry) entry.refs += 1;
        return url;
    }
    const request = fetch(briefingArtifactUrl(sessionId, path), {
        method: "GET",
        headers: buildBackendAuthHeaders("GET"),
        credentials: "omit",
        cache: "no-store",
    }).then(async (response) => {
        if (!response.ok) throw new Error(`briefing media ${response.status}`);
        const blob = await response.blob();
        const url = URL.createObjectURL(blob);
        briefingUrls.set(key, { url, refs: 0 });
        return url;
    }).finally(() => {
        briefingInflight.delete(key);
    });
    briefingInflight.set(key, request);
    const url = await request;
    const entry = briefingUrls.get(key);
    if (entry) entry.refs += 1;
    return url;
}

export function releaseBriefingMediaUrl(sessionId: string, path: string) {
    const key = `${sessionId}:${path}`;
    const entry = briefingUrls.get(key);
    if (!entry) return;
    entry.refs = Math.max(0, entry.refs - 1);
}

export async function blobFromPreviewUrl(url: string): Promise<Blob> {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`preview fetch ${response.status}`);
    return response.blob();
}

export function fileFromAssetBlob(blob: Blob, path: string, fallbackName: string, kind: AssetSpaceKind) {
    const name = path.split("/").pop()?.split("?")[0] || fallbackName;
    const type = blob.type && blob.type !== "application/octet-stream" ? blob.type : mimeForAssetKind(kind, name);
    return new File([blob], name, { type });
}

export async function fileFromVimaxArtifact(sessionId: string, path: string, fallbackName: string, kind: AssetSpaceKind) {
    const url = await acquireCachedArtifactMediaUrl(sessionId, path);
    try {
        return fileFromAssetBlob(await blobFromPreviewUrl(url), path, fallbackName, kind);
    } finally {
        releaseCachedArtifactMediaUrl(sessionId, path);
    }
}

export async function fileFromBriefingArtifact(sessionId: string, path: string, fallbackName: string, kind: AssetSpaceKind) {
    const url = await acquireBriefingMediaUrl(sessionId, path);
    try {
        return fileFromAssetBlob(await blobFromPreviewUrl(url), path, fallbackName, kind);
    } finally {
        releaseBriefingMediaUrl(sessionId, path);
    }
}

function mimeForAssetKind(kind: AssetSpaceKind, name: string) {
    const lower = name.toLowerCase();
    if (kind === "video") return "video/mp4";
    if (kind === "audio") return lower.endsWith(".wav") ? "audio/wav" : "audio/mpeg";
    if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
    if (lower.endsWith(".webp")) return "image/webp";
    return "image/png";
}
