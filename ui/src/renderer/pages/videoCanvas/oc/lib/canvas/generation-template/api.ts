import { httpRequest } from "@/common/adapter/httpBridge";

import type {
    GenerationTemplateDetail,
    GenerationTemplateEventType,
    GenerationTemplateListResult,
    GenerationTemplatePublishInput,
} from "./types";

const BASE = "/api/vimax";

function query(params: Record<string, string | number | undefined | null>): string {
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
        if (value === undefined || value === null || value === "") continue;
        search.set(key, String(value));
    }
    const qs = search.toString();
    return qs ? `?${qs}` : "";
}

export async function listGenerationTemplates(params?: {
    page?: number;
    pageSize?: number;
    keyword?: string;
    category?: string;
    origin?: string;
    sort?: string;
    nodeType?: string;
}): Promise<GenerationTemplateListResult> {
    return httpRequest<GenerationTemplateListResult>(
        "GET",
        `${BASE}/generation-templates${query({
            page: params?.page,
            pageSize: params?.pageSize,
            keyword: params?.keyword,
            category: params?.category,
            origin: params?.origin,
            sort: params?.sort,
            nodeType: params?.nodeType,
        })}`,
    );
}

export async function listMyGenerationTemplates(params?: {
    page?: number;
    pageSize?: number;
    status?: string;
}): Promise<GenerationTemplateListResult> {
    return httpRequest<GenerationTemplateListResult>(
        "GET",
        `${BASE}/generation-templates/mine${query({
            page: params?.page,
            pageSize: params?.pageSize,
            status: params?.status,
        })}`,
    );
}

export async function getGenerationTemplateDetail(id: number): Promise<GenerationTemplateDetail> {
    return httpRequest<GenerationTemplateDetail>("GET", `${BASE}/generation-templates/${id}`);
}

export async function recordGenerationTemplateEvent(id: number, type: GenerationTemplateEventType): Promise<void> {
    await httpRequest("POST", `${BASE}/generation-templates/${id}/events`, { type });
}

export function fireGenerationTemplateEvent(id: number | undefined, type: GenerationTemplateEventType) {
    if (!id || id <= 0) return;
    void recordGenerationTemplateEvent(id, type).catch(() => undefined);
}

export async function publishGenerationTemplateFromCanvas(body: GenerationTemplatePublishInput): Promise<GenerationTemplateDetail> {
    return httpRequest<GenerationTemplateDetail>("POST", `${BASE}/generation-templates/publish-from-canvas`, body);
}
