import type { CanvasNodeType, CanvasVideoEditOperation } from "@oc/types/canvas";

export type GenerationTemplateOrigin = "official" | "ugc";
export type GenerationTemplateStatus = "draft" | "pending" | "published" | "offline" | "deleted";
export type GenerationTemplateApplyPolicy = "replace" | "merge" | "slot-fill";
export type GenerationTemplateNodeType = "image" | "video";
export type GenerationTemplateAssetRole = "start_frame" | "end_frame" | "character" | "style" | "motion";
export type GenerationTemplateSlotKind = "text" | "asset" | "enum";
export type GenerationTemplateEventType = "impression" | "apply" | "generate" | "succeed";
export type GenerationTemplateLegalOperation = Extract<CanvasVideoEditOperation, "text_to_video" | "image_to_video" | "reference_to_video">;

export type GenerationTemplateAuthor = {
    id: number;
    name: string;
    avatarUrl?: string | null;
};

export type GenerationTemplateTarget = {
    nodeTypes: GenerationTemplateNodeType[];
    operations: string[];
};

export type GenerationTemplatePrompt = {
    body: string;
    negative?: string;
    adapters?: Record<string, string>;
};

export type GenerationTemplateSlot = {
    id: string;
    kind: GenerationTemplateSlotKind | string;
    label: string;
    required: boolean;
    default?: string;
    options?: string[];
};

export type GenerationTemplateModelIntent = {
    preferred: string;
    fallbacks?: string[];
    constraints?: Record<string, string>;
};

export type GenerationTemplateAsset = {
    id?: number;
    role: GenerationTemplateAssetRole | string;
    sort?: number;
    optional?: boolean;
    url: string;
    objectKey?: string;
    mimeType?: string;
    sizeBytes?: number | null;
    sha256?: string;
};

export type TemplateInputKind = "text" | "start" | "start-end" | "reference";

export type GenerationTemplateListItem = {
    id: number;
    slug: string;
    version: string;
    title: string;
    titleEn?: string;
    job: string;
    jobEn?: string;
    category: string;
    origin: GenerationTemplateOrigin | string;
    status: GenerationTemplateStatus | string;
    featuredRank?: number;
    tags: string[];
    applyPolicy: GenerationTemplateApplyPolicy | string;
    assetRoles?: string[];
    coverUrl?: string | null;
    previewUrl?: string | null;
    estimatedCredits: number;
    impressionCount?: number;
    applyCount?: number;
    generateCount?: number;
    succeedCount?: number;
    rejectReason?: string | null;
    submittedAt?: string;
    publishedAt?: string | null;
    author: GenerationTemplateAuthor;
    isMine?: boolean;
    target: GenerationTemplateTarget;
    modelIntent: GenerationTemplateModelIntent;
};

export type GenerationTemplateDetail = GenerationTemplateListItem & {
    prompt: GenerationTemplatePrompt;
    slots: GenerationTemplateSlot[];
    assets: GenerationTemplateAsset[];
    coverObjectKey?: string | null;
    previewObjectKey?: string | null;
    remixOfId?: number | null;
    sourceNodeId?: string | null;
};

export type GenerationTemplateListResult = {
    total: number;
    page: number;
    pageSize: number;
    list: GenerationTemplateListItem[];
};

export type GenerationTemplatePublishInput = {
    slug: string;
    version?: string;
    title: string;
    titleEn?: string;
    job: string;
    jobEn?: string;
    category?: string;
    origin?: string;
    target: GenerationTemplateTarget;
    prompt: GenerationTemplatePrompt;
    slots?: GenerationTemplateSlot[];
    modelIntent?: GenerationTemplateModelIntent;
    tags?: string[];
    applyPolicy?: string;
    coverUrl?: string;
    previewUrl?: string;
    estimatedCredits?: number;
    remixOfId?: number;
    sourceNodeId?: string;
    assets?: GenerationTemplateAsset[];
};

export type AppliedTemplateStamp = {
    id: number;
    slug?: string;
    version: string;
    appliedAt: string;
    degraded: boolean;
    degradeReason?: string;
    remixOf?: number;
    promptBody: string;
    adapters?: Record<string, string>;
    slots: GenerationTemplateSlot[];
};

export type CompiledAsset = {
    role: GenerationTemplateAssetRole;
    url: string;
    optional: boolean;
};

export type CompiledGenerationTemplate = {
    template: GenerationTemplateDetail;
    nodeType: CanvasNodeType;
    prompt: string;
    negative?: string;
    slotValues: Record<string, string>;
    model?: string;
    operation?: GenerationTemplateLegalOperation;
    startFrame?: CompiledAsset;
    endFrame?: CompiledAsset;
    referenceAssets: CompiledAsset[];
    degraded: boolean;
    degradeReason?: string;
    applyPolicy: GenerationTemplateApplyPolicy;
    seconds?: string;
    size?: string;
    stamp: AppliedTemplateStamp;
};

export function templateInputKind(operations: string[] | undefined, assetRoles?: string[]): TemplateInputKind {
    const roles = new Set((assetRoles || []).map((item) => item.trim().toLowerCase()));
    const hasStart = roles.has("start_frame");
    const hasEnd = roles.has("end_frame");
    const hasRef = [...roles].some((role) => role === "character" || role === "style" || role === "motion");
    if (hasStart && hasEnd) return "start-end";
    if (hasStart || hasEnd) return "start";
    if (hasRef) return "reference";
    const ops = (operations || []).map((item) => item.trim().toLowerCase());
    if (ops.some((item) => item.includes("reference") || item === "r2v")) return "reference";
    if (ops.some((item) => item.includes("image_to") || item === "i2v" || item === "i2i")) return "start";
    return "text";
}

export function isFrameAssetRole(role: string): role is "start_frame" | "end_frame" {
    return role === "start_frame" || role === "end_frame";
}

export function isReferenceAssetRole(role: string): role is "character" | "style" | "motion" {
    return role === "character" || role === "style" || role === "motion";
}
