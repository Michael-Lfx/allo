import type { BriefingSessionSummary } from "@renderer/pages/videoGeneration/briefing/api";
import type { SessionSummary } from "@renderer/pages/videoGeneration/types";
import { canvasMediaUrl, type GenerationTaskView } from "@renderer/pages/videoCanvas/api";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { CanvasNodeType, type CanvasNodeData } from "@oc/types/canvas";

export const ASSET_SPACE_SOURCES = ["canvas", "drama", "generate", "briefing"] as const;
export type AssetSpaceSource = (typeof ASSET_SPACE_SOURCES)[number];

export const ASSET_SPACE_KINDS = ["image", "video", "audio"] as const;
export type AssetSpaceKind = (typeof ASSET_SPACE_KINDS)[number];

export const ASSET_SPACE_DRAMA_CATEGORIES = ["film", "character", "environment", "prop"] as const;
export type AssetSpaceDramaCategory = (typeof ASSET_SPACE_DRAMA_CATEGORIES)[number];

const SKIP_DRAMA_STILL_RE = /_raw|_generation_prompt|_atmosphere/i;
const WORKFLOW_RELATIVE_RE = /(?:^|\/)((?:idea2video|script2video|novel2video|action2video)\/.+)$/i;
const FILM_ROOT_ASSET_RE = /^(character_portraits|environments|props|world_assets)(\/|$)/i;
const SKIP_PORTRAIT_REGISTRY_KEYS = new Set(["environments", "props"]);

type DramaSessionRef = Pick<SessionSummary, "id" | "title" | "updated_at" | "created_at" | "cover" | "final_video" | "workflow">;

export type AssetSpacePreview =
    | { type: "http"; url: string }
    | { type: "node"; nodeId: string }
    | { type: "vimax"; sessionId: string; path: string }
    | { type: "briefing"; sessionId: string; path: string };

export type AssetSpaceAction =
    | { type: "focus"; nodeId: string }
    | { type: "insert-media"; mediaId: string; kind: AssetSpaceKind }
    | { type: "insert-vimax"; sessionId: string; path: string; kind: AssetSpaceKind }
    | { type: "insert-briefing"; sessionId: string; path: string; kind: AssetSpaceKind };

export type AssetSpaceItem = {
    id: string;
    source: AssetSpaceSource;
    kind: AssetSpaceKind;
    title: string;
    subtitle?: string;
    category?: AssetSpaceDramaCategory;
    updatedAt: number;
    preview: AssetSpacePreview;
    action: AssetSpaceAction;
};

export function assetSpaceTimestamp(value: string | number | null | undefined): number {
    if (typeof value === "number" && Number.isFinite(value)) return value < 1e12 ? value * 1000 : value;
    if (typeof value === "string" && value.trim()) {
        const parsed = Date.parse(value);
        return Number.isFinite(parsed) ? parsed : 0;
    }
    return 0;
}

export function canvasNodeAssetKind(node: CanvasNodeData): AssetSpaceKind | null {
    if (node.type === CanvasNodeType.Image) return "image";
    if (node.type === CanvasNodeType.Video) return "video";
    if (node.type === CanvasNodeType.Audio) return "audio";
    return null;
}

export function canvasNodeHasMedia(node: CanvasNodeData): boolean {
    return Boolean(node.metadata?.mediaId || node.metadata?.storageKey || node.metadata?.content?.trim());
}

export function assetSpaceItemsFromCanvasNodes(nodes: CanvasNodeData[]): AssetSpaceItem[] {
    return nodes.flatMap((node) => {
        const kind = canvasNodeAssetKind(node);
        if (!kind || !canvasNodeHasMedia(node)) return [];
        return [{
            id: `canvas:${node.id}`,
            source: "canvas" as const,
            kind,
            title: node.title || node.metadata?.prompt || fallbackTitle(kind),
            subtitle: node.metadata?.prompt && node.title ? node.metadata.prompt : undefined,
            updatedAt: 0,
            preview: { type: "node", nodeId: node.id },
            action: { type: "focus", nodeId: node.id },
        }];
    });
}

export function assetSpaceItemsFromDrama(sessions: SessionSummary[]): AssetSpaceItem[] {
    return sessions.flatMap((session) => {
        const updatedAt = assetSpaceTimestamp(session.updated_at ?? session.created_at);
        const title = session.title?.trim() || canvasT("videoCanvas.asset.untitledDrama", "未命名短剧");
        const items: AssetSpaceItem[] = [];
        const cover = session.cover?.trim();
        if (cover) {
            items.push({
                id: `drama:${session.id}:cover`,
                source: "drama",
                kind: "image",
                title,
                subtitle: canvasT("videoCanvas.asset.subtitleCover", "封面"),
                category: "film",
                updatedAt,
                preview: { type: "vimax", sessionId: session.id, path: cover },
                action: { type: "insert-vimax", sessionId: session.id, path: cover, kind: "image" },
            });
        }
        const video = session.final_video?.trim();
        if (video) {
            items.push({
                id: `drama:${session.id}:video`,
                source: "drama",
                kind: "video",
                title,
                subtitle: canvasT("videoCanvas.asset.subtitleFinal", "成片"),
                category: "film",
                updatedAt,
                preview: { type: "vimax", sessionId: session.id, path: video },
                action: { type: "insert-vimax", sessionId: session.id, path: video, kind: "video" },
            });
        }
        return items;
    });
}

export function generateTaskKind(mode: string | null | undefined): AssetSpaceKind {
    if (mode === "image") return "image";
    if (mode === "audio") return "audio";
    return "video";
}

export function assetSpaceItemsFromGenerate(tasks: GenerationTaskView[]): AssetSpaceItem[] {
    return tasks.flatMap((task) => {
        if (task.status !== "succeeded" || !task.result_media_id) return [];
        const kind = generateTaskKind(task.mode);
        const title = task.prompt?.trim()?.slice(0, 42) || canvasT("videoCanvas.asset.untitledGenerate", "视频生成");
        const updatedAt = assetSpaceTimestamp(task.updated_at || task.created_at);
        const items: AssetSpaceItem[] = [{
            id: `generate:${task.task_id}:result`,
            source: "generate",
            kind,
            title,
            subtitle: task.model || undefined,
            updatedAt,
            preview: { type: "http", url: canvasMediaUrl(task.result_media_id) },
            action: { type: "insert-media", mediaId: task.result_media_id, kind },
        }];
        if (task.first_frame_media_id && task.first_frame_media_id !== task.result_media_id) {
            items.push({
                id: `generate:${task.task_id}:first`,
                source: "generate",
                kind: "image",
                title,
                subtitle: canvasT("videoCanvas.asset.subtitleFirst", "首帧"),
                updatedAt,
                preview: { type: "http", url: canvasMediaUrl(task.first_frame_media_id) },
                action: { type: "insert-media", mediaId: task.first_frame_media_id, kind: "image" },
            });
        }
        if (task.last_frame_media_id && task.last_frame_media_id !== task.result_media_id && task.last_frame_media_id !== task.first_frame_media_id) {
            items.push({
                id: `generate:${task.task_id}:last`,
                source: "generate",
                kind: "image",
                title,
                subtitle: canvasT("videoCanvas.asset.subtitleLast", "尾帧"),
                updatedAt,
                preview: { type: "http", url: canvasMediaUrl(task.last_frame_media_id) },
                action: { type: "insert-media", mediaId: task.last_frame_media_id, kind: "image" },
            });
        }
        return items;
    });
}

export function briefingVideoPath(session: BriefingSessionSummary): string {
    const explicit = session.final_video?.trim();
    if (explicit) return explicit.replace(/^\/+/, "");
    if (session.status === "succeeded") return "briefing.mp4";
    return "";
}

export function assetSpaceItemsFromBriefing(sessions: BriefingSessionSummary[]): AssetSpaceItem[] {
    return sessions.flatMap((session) => {
        const path = briefingVideoPath(session);
        if (!path) return [];
        return [{
            id: `briefing:${session.id}:video`,
            source: "briefing" as const,
            kind: "video" as const,
            title: session.title?.trim() || canvasT("videoCanvas.asset.untitledBriefing", "资讯播报"),
            subtitle: session.stage || undefined,
            updatedAt: assetSpaceTimestamp(session.updated_at || session.created_at),
            preview: { type: "briefing" as const, sessionId: session.id, path },
            action: { type: "insert-briefing" as const, sessionId: session.id, path, kind: "video" as const },
        }];
    });
}

export function filterAssetSpaceItems(
    items: AssetSpaceItem[],
    options: { source?: AssetSpaceSource | "all"; kind?: AssetSpaceKind | "all"; category?: AssetSpaceDramaCategory | "all"; query?: string },
): AssetSpaceItem[] {
    const query = options.query?.trim().toLowerCase() || "";
    return items.filter((item) => {
        if (options.source && options.source !== "all" && item.source !== options.source) return false;
        if (options.kind && options.kind !== "all" && item.kind !== options.kind) return false;
        if (options.category && options.category !== "all" && item.category !== options.category) return false;
        if (!query) return true;
        return [item.title, item.subtitle].filter(Boolean).join(" ").toLowerCase().includes(query);
    });
}

export function countAssetSpaceBySource(items: AssetSpaceItem[]): Record<AssetSpaceSource, number> {
    return {
        canvas: items.filter((item) => item.source === "canvas").length,
        drama: items.filter((item) => item.source === "drama").length,
        generate: items.filter((item) => item.source === "generate").length,
        briefing: items.filter((item) => item.source === "briefing").length,
    };
}

export function countAssetSpaceByKind(items: AssetSpaceItem[]): Record<AssetSpaceKind | "all", number> {
    return {
        all: items.length,
        image: items.filter((item) => item.kind === "image").length,
        video: items.filter((item) => item.kind === "video").length,
        audio: items.filter((item) => item.kind === "audio").length,
    };
}

export function countAssetSpaceByDramaCategory(items: AssetSpaceItem[]): Record<AssetSpaceDramaCategory, number> {
    return {
        film: items.filter((item) => item.category === "film").length,
        character: items.filter((item) => item.category === "character").length,
        environment: items.filter((item) => item.category === "environment").length,
        prop: items.filter((item) => item.category === "prop").length,
    };
}

export function dramaFilmPrefix(session: Pick<SessionSummary, "cover" | "final_video" | "workflow">): string {
    const fromCover = dirnamePath(session.cover);
    if (fromCover) return fromCover;
    const fromVideo = dirnamePath(session.final_video);
    if (fromVideo) return fromVideo;
    return session.workflow || "idea2video";
}

export function dramaSessionLikelyHasPlates(session: SessionSummary): boolean {
    if (session.cover?.trim() || session.final_video?.trim()) return true;
    const hint = `${session.stage || ""} ${session.status || ""}`;
    return /portrait|world|asset|storyboard|shot|render|succeed|character|environment|prop|video/i.test(hint);
}

export function vimaxRelativeArtifactPath(raw: string | null | undefined): string | null {
    const normalized = (raw || "").trim().replace(/\\/g, "/").replace(/\/{2,}/g, "/");
    if (!normalized) return null;
    if (SKIP_DRAMA_STILL_RE.test(normalized) || /(^|\/)look_plate\.png$/i.test(normalized)) return null;
    const workflow = normalized.match(WORKFLOW_RELATIVE_RE);
    if (workflow?.[1]) return decodeURIComponent(workflow[1]);
    const lower = normalized.toLowerCase();
    for (const marker of ["character_portraits/", "environments/", "props/", "world_assets/", "references/", "cameo/", "action2video/"]) {
        const at = lower.lastIndexOf(marker);
        if (at >= 0) return normalized.slice(at);
    }
    if (/^[a-zA-Z]:/.test(normalized) || normalized.startsWith("/")) return null;
    return normalized.replace(/^\.\//, "");
}

export function qualifyDramaArtifactPath(prefix: string, raw: string | null | undefined): string | null {
    const relative = vimaxRelativeArtifactPath(raw);
    if (!relative) return null;
    const filmPrefix = (prefix || "").replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
    if (!filmPrefix) return relative;
    if (relative === filmPrefix || relative.startsWith(`${filmPrefix}/`)) return relative;
    if (WORKFLOW_RELATIVE_RE.test(relative) || WORKFLOW_RELATIVE_RE.test(`/${relative}`)) return relative;
    if (!FILM_ROOT_ASSET_RE.test(relative)) return relative;
    return `${filmPrefix}/${relative}`;
}

export function dramaRegistryJsonPaths(session: Pick<SessionSummary, "cover" | "final_video" | "workflow">, filename: string): string[] {
    const prefix = dramaFilmPrefix(session);
    const workflow = session.workflow || "idea2video";
    return [...new Set([`${prefix}/${filename}`, `${workflow}/${filename}`, filename])]
        .map((path) => path.replace(/\\/g, "/").replace(/\/{2,}/g, "/").replace(/^\/+/, ""))
        .filter(Boolean);
}

export function selectDramaSessionsForPlates(sessions: SessionSummary[], cap = 40): SessionSummary[] {
    const sorted = sessions.toSorted((left, right) => assetSpaceTimestamp(right.updated_at ?? right.created_at) - assetSpaceTimestamp(left.updated_at ?? left.created_at) || left.id.localeCompare(right.id));
    const likely: SessionSummary[] = [];
    const rest: SessionSummary[] = [];
    for (const session of sorted) {
        if (dramaSessionLikelyHasPlates(session)) likely.push(session);
        else rest.push(session);
    }
    return [...likely, ...rest].slice(0, cap);
}

export function assetSpaceItemsFromPortraitRegistry(session: DramaSessionRef, registry: unknown): AssetSpaceItem[] {
    const portraits = unwrapDramaRegistry(registry);
    if (!portraits || typeof portraits !== "object" || Array.isArray(portraits)) return [];
    const prefix = dramaFilmPrefix(session);
    const updatedAt = assetSpaceTimestamp(session.updated_at ?? session.created_at);
    const sessionTitle = session.title?.trim() || canvasT("videoCanvas.asset.untitledDrama", "未命名短剧");
    const items: AssetSpaceItem[] = [];
    const seen = new Set<string>();
    for (const [name, views] of Object.entries(portraits as Record<string, unknown>)) {
        if (SKIP_PORTRAIT_REGISTRY_KEYS.has(name)) continue;
        const character = name.trim() || canvasT("videoCanvas.asset.categoryCharacter", "人物");
        if (!views || typeof views !== "object" || Array.isArray(views)) {
            pushDramaItem(items, seen, session.id, prefix, updatedAt, character, sessionTitle, "character", "image", registryPath(views), "sheet");
            continue;
        }
        const rec = views as Record<string, unknown>;
        const sheet = registryPath(rec.sheet) || registryPath(rec);
        pushDramaItem(items, seen, session.id, prefix, updatedAt, character, sessionTitle, "character", "image", sheet, "sheet");
        if (!sheet) {
            pushDramaItem(items, seen, session.id, prefix, updatedAt, character, sessionTitle, "character", "image", registryPath(rec.front), "front");
        }
        pushDramaItem(items, seen, session.id, prefix, updatedAt, character, sessionTitle, "character", "image", registryPath(rec.cameo), "cameo");
        pushDramaItem(items, seen, session.id, prefix, updatedAt, character, sessionTitle, "character", "audio", registryPath(rec.voice_ref), "voice");
    }
    return items;
}

export function assetSpaceItemsFromWorldRegistry(session: DramaSessionRef, registry: unknown): AssetSpaceItem[] {
    const world = unwrapDramaRegistry(registry);
    if (!world || typeof world !== "object" || Array.isArray(world)) return [];
    const rec = world as Record<string, unknown>;
    const prefix = dramaFilmPrefix(session);
    const updatedAt = assetSpaceTimestamp(session.updated_at ?? session.created_at);
    const sessionTitle = session.title?.trim() || canvasT("videoCanvas.asset.untitledDrama", "未命名短剧");
    const items: AssetSpaceItem[] = [];
    const seen = new Set<string>();
    collectNamedRegistryGroup(items, seen, session.id, prefix, updatedAt, sessionTitle, rec.environments, "environment");
    collectNamedRegistryGroup(items, seen, session.id, prefix, updatedAt, sessionTitle, rec.props, "prop");
    return items;
}

export function assetSpaceItemsFromDramaCameos(
    session: DramaSessionRef,
    photos: Array<{ id: string; rel_path?: string | null; character_name?: string | null }>,
): AssetSpaceItem[] {
    const prefix = dramaFilmPrefix(session);
    const updatedAt = assetSpaceTimestamp(session.updated_at ?? session.created_at);
    const sessionTitle = session.title?.trim() || canvasT("videoCanvas.asset.untitledDrama", "未命名短剧");
    const items: AssetSpaceItem[] = [];
    const seen = new Set<string>();
    for (const photo of photos) {
        const path = qualifyDramaArtifactPath(prefix, photo.rel_path);
        if (!path || !isImageArtifactName(path)) continue;
        const category = cameoDramaCategory(path);
        if (!category) continue;
        const title = photo.character_name?.trim() || fileStem(path);
        pushDramaItem(items, seen, session.id, prefix, updatedAt, title, sessionTitle, category, "image", path, "cameo", photo.id);
    }
    return items;
}

export function assetSpaceItemsFromDramaActionAssets(
    session: DramaSessionRef,
    assets: { character?: string | null; reference_video?: string | null },
): AssetSpaceItem[] {
    const prefix = dramaFilmPrefix(session);
    const updatedAt = assetSpaceTimestamp(session.updated_at ?? session.created_at);
    const sessionTitle = session.title?.trim() || canvasT("videoCanvas.asset.untitledDrama", "未命名短剧");
    const items: AssetSpaceItem[] = [];
    const seen = new Set<string>();
    pushDramaItem(items, seen, session.id, prefix, updatedAt, sessionTitle, sessionTitle, "character", "image", assets.character, "action-character");
    pushDramaItem(items, seen, session.id, prefix, updatedAt, sessionTitle, sessionTitle, "film", "video", assets.reference_video, "action-ref");
    return items;
}

function collectNamedRegistryGroup(
    items: AssetSpaceItem[],
    seen: Set<string>,
    sessionId: string,
    prefix: string,
    updatedAt: number,
    sessionTitle: string,
    group: unknown,
    category: Exclude<AssetSpaceDramaCategory, "film">,
) {
    if (!group || typeof group !== "object") return;
    if (Array.isArray(group)) {
        for (const entry of group) {
            const name = namedRegistryLabel(entry) || canvasT(category === "environment" ? "videoCanvas.asset.categoryEnvironment" : "videoCanvas.asset.categoryProp", category === "environment" ? "环境" : "道具");
            pushDramaItem(items, seen, sessionId, prefix, updatedAt, name, sessionTitle, category, "image", registryPath(entry), category);
        }
        return;
    }
    for (const [name, entry] of Object.entries(group as Record<string, unknown>)) {
        pushDramaItem(items, seen, sessionId, prefix, updatedAt, name.trim() || namedRegistryLabel(entry), sessionTitle, category, "image", registryPath(entry), category);
    }
}

function pushDramaItem(
    items: AssetSpaceItem[],
    seen: Set<string>,
    sessionId: string,
    prefix: string,
    updatedAt: number,
    title: string,
    sessionTitle: string,
    category: AssetSpaceDramaCategory,
    kind: AssetSpaceKind,
    rawPath: string | null | undefined,
    role: string,
    extraId?: string,
) {
    const path = qualifyDramaArtifactPath(prefix, rawPath);
    if (!path) return;
    if (kind === "image" && !isImageArtifactName(path)) return;
    if (kind === "audio" && !isAudioArtifactName(path)) return;
    if (kind === "video" && !isVideoArtifactName(path)) return;
    const key = `${sessionId}:${path}`;
    if (seen.has(key)) return;
    seen.add(key);
    items.push({
        id: `drama:${sessionId}:${role}:${extraId || path}`,
        source: "drama",
        kind,
        title: title.trim() || sessionTitle,
        subtitle: `${dramaCategoryLabel(category)} · ${sessionTitle}`,
        category,
        updatedAt,
        preview: { type: "vimax", sessionId, path },
        action: { type: "insert-vimax", sessionId, path, kind },
    });
}

function dramaCategoryLabel(category: AssetSpaceDramaCategory) {
    if (category === "character") return canvasT("videoCanvas.asset.categoryCharacter", "人物");
    if (category === "environment") return canvasT("videoCanvas.asset.categoryEnvironment", "环境");
    if (category === "prop") return canvasT("videoCanvas.asset.categoryProp", "道具");
    return canvasT("videoCanvas.asset.categoryFilm", "成片");
}

function cameoDramaCategory(path: string): AssetSpaceDramaCategory | null {
    const lower = path.replace(/\\/g, "/").toLowerCase();
    if (/\/by_category\/environment\//.test(lower) || /\/environments\//.test(lower)) return "environment";
    if (/\/by_category\/prop\//.test(lower) || /\/props\//.test(lower)) return "prop";
    if (/\/by_category\/style\//.test(lower)) return null;
    return "character";
}

function unwrapDramaRegistry(value: unknown): unknown {
    if (!value || typeof value !== "object" || Array.isArray(value)) return value;
    const rec = value as Record<string, unknown>;
    if (("success" in rec || "code" in rec) && rec.data && typeof rec.data === "object") return rec.data;
    return value;
}

function registryPath(entry: unknown): string {
    if (typeof entry === "string") return entry.trim();
    if (!entry || typeof entry !== "object") return "";
    const rec = entry as Record<string, unknown>;
    for (const key of ["path", "file", "abs", "rel", "png", "image"]) {
        const value = rec[key];
        if (typeof value === "string" && value.trim()) return value.trim();
    }
    return "";
}

function namedRegistryLabel(entry: unknown): string {
    if (!entry || typeof entry !== "object") return "";
    const rec = entry as Record<string, unknown>;
    for (const key of ["name", "slugline", "identifier_in_scene", "title"]) {
        const value = rec[key];
        if (typeof value === "string" && value.trim()) return value.trim();
    }
    return fileStem(registryPath(entry));
}

function dirnamePath(path: string | null | undefined): string {
    const normalized = (path || "").trim().replace(/\\/g, "/");
    const index = normalized.lastIndexOf("/");
    return index > 0 ? normalized.slice(0, index) : "";
}

function fileStem(path: string) {
    const base = path.replace(/\\/g, "/").split("/").pop() || path;
    return base.replace(/\.[^.]+$/, "").replace(/_/g, " ");
}

function isImageArtifactName(path: string) {
    return /\.(png|jpe?g|gif|webp|bmp)$/i.test(path);
}

function isAudioArtifactName(path: string) {
    return /\.(mp3|wav|m4a|aac|ogg|oga|flac|opus)$/i.test(path);
}

function isVideoArtifactName(path: string) {
    return /\.(mp4|webm|mov|avi|mkv)$/i.test(path);
}

function fallbackTitle(kind: AssetSpaceKind) {
    if (kind === "video") return canvasT("videoCanvas.asset.videoNode", "视频节点");
    if (kind === "audio") return canvasT("videoCanvas.asset.audioNode", "音频节点");
    return canvasT("videoCanvas.asset.imageNode", "图片节点");
}
