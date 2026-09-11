import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { CanvasNodeType } from "@oc/types/canvas";

import type {
    CompiledAsset,
    CompiledGenerationTemplate,
    GenerationTemplateApplyPolicy,
    GenerationTemplateAsset,
    GenerationTemplateAssetRole,
    GenerationTemplateDetail,
    GenerationTemplateLegalOperation,
    GenerationTemplateSlot,
    GenerationTemplateTarget,
} from "./types";
import { isReferenceAssetRole } from "./types";

export type CompileGenerationTemplateInput = {
    slotValues?: Record<string, string>;
    availableModels: string[];
    selectedNodeType?: CanvasNodeType | string;
    currentModel?: string;
    applyPolicy?: GenerationTemplateApplyPolicy | string;
};

const SLOT_TOKEN = /\{\{([a-zA-Z0-9_-]+)\}\}/g;

export function promptFamily(model: string): string | undefined {
    const n = model.trim().toLowerCase();
    if (!n) return undefined;
    if (n.includes("kling")) return "kling";
    if (n.includes("seedance") || n.includes("seedream")) return "seedance";
    if (n.includes("sora")) return "sora";
    if (n.includes("wan")) return "wan";
    if (n.includes("hailuo") || n.includes("minimax")) return "minimax";
    if (n.includes("veo")) return "veo";
    return undefined;
}

const PRIMARY_SLOT = /^(subject|scene|prompt|hero|主角|主体|场景)$/i;

export function fillSlotValues(slots: GenerationTemplateSlot[], values: Record<string, string> = {}): Record<string, string> {
    const filled: Record<string, string> = {};
    for (const slot of slots) {
        if (slot.kind === "asset") continue;
        const raw = values[slot.id] ?? slot.default ?? "";
        filled[slot.id] = String(raw).trim();
    }
    return filled;
}

export function rewriteSlotsFromInstruction(slots: GenerationTemplateSlot[], current: Record<string, string> | undefined, instruction: string): Record<string, string> {
    const next = fillSlotValues(slots, current);
    const text = instruction.trim();
    if (!text) return next;
    const lower = text.toLowerCase();
    let primaryText = text;
    for (const slot of slots) {
        if (slot.kind !== "enum" || !slot.options?.length) continue;
        const hit = slot.options.find((option) => option && lower.includes(option.trim().toLowerCase()));
        if (!hit) continue;
        next[slot.id] = hit;
        primaryText = primaryText.replace(new RegExp(escapeRegExp(hit), "ig"), " ");
    }
    primaryText = primaryText.replace(/\s+/g, " ").trim();
    const textSlots = slots.filter((slot) => slot.kind !== "asset" && slot.kind !== "enum");
    const primary = textSlots.find((slot) => PRIMARY_SLOT.test(slot.id) || PRIMARY_SLOT.test(slot.label))
        || textSlots.find((slot) => slot.required)
        || textSlots[0];
    if (primary) next[primary.id] = primaryText || text;
    return next;
}

function escapeRegExp(value: string) {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function compilePromptBody(body: string, adapters: Record<string, string> | undefined, model: string | undefined, slotValues: Record<string, string>): string {
    const family = model ? promptFamily(model) : undefined;
    const template = (family && adapters?.[family]?.trim()) || body;
    return template.replace(SLOT_TOKEN, (_, id: string) => slotValues[id] ?? "");
}

export function negotiateTemplateModel(preferred: string, fallbacks: string[] | undefined, available: string[], current?: string): { model?: string; degraded: boolean; reason?: string } {
    const pool = available.map((item) => item.trim()).filter(Boolean);
    const exact = (token: string) => pool.find((item) => item.toLowerCase() === token.toLowerCase());
    const fuzzy = (token: string) => {
        const needle = token.trim().toLowerCase();
        if (!needle) return undefined;
        return pool.find((item) => item.toLowerCase().includes(needle) || needle.includes(item.toLowerCase()));
    };
    const pick = (token: string) => exact(token) || fuzzy(token);
    const preferredHit = pick(preferred);
    if (preferredHit) return { model: preferredHit, degraded: false };
    for (const fallback of fallbacks || []) {
        const hit = pick(fallback);
        if (hit) return { model: hit, degraded: true, reason: "model-fallback" };
    }
    const currentHit = current ? pick(current) || (pool.includes(current) ? current : undefined) : undefined;
    if (currentHit) return { model: currentHit, degraded: true, reason: "model-fallback" };
    if (pool[0]) return { model: pool[0], degraded: true, reason: "model-fallback" };
    const leftover = preferred.trim();
    if (leftover) return { model: leftover, degraded: true, reason: "model-fallback" };
    return { degraded: true, reason: "model-fallback" };
}

function normalizeApplyPolicy(raw: string | undefined): GenerationTemplateApplyPolicy {
    if (raw === "merge" || raw === "slot-fill") return raw;
    return "replace";
}

function constraintValue(constraints: Record<string, string> | undefined, keys: string[]): string | undefined {
    if (!constraints) return undefined;
    for (const key of keys) {
        const value = String(constraints[key] || "").trim();
        if (value) return value;
    }
    return undefined;
}

function compiledSeconds(constraints: Record<string, string> | undefined): string | undefined {
    const raw = constraintValue(constraints, ["seconds", "duration", "durationSecs", "duration_secs"]);
    if (!raw) return undefined;
    const numeric = Number.parseFloat(raw.replace(/s$/i, ""));
    if (!Number.isFinite(numeric) || numeric <= 0) return undefined;
    return String(Math.round(numeric));
}

function compiledSize(constraints: Record<string, string> | undefined): string | undefined {
    return constraintValue(constraints, ["size", "aspectRatio", "aspect_ratio", "ratio"]);
}

function targetNodeType(target: GenerationTemplateTarget, selected?: CanvasNodeType | string): CanvasNodeType {
    const allowed = new Set((target.nodeTypes || []).map((item) => item.trim().toLowerCase()));
    const selectedKey = String(selected || "").toLowerCase();
    if (selectedKey === "video" && (allowed.size === 0 || allowed.has("video"))) return CanvasNodeType.Video;
    if (selectedKey === "image" && (allowed.size === 0 || allowed.has("image"))) return CanvasNodeType.Image;
    if (allowed.has("video")) return CanvasNodeType.Video;
    if (allowed.has("image")) return CanvasNodeType.Image;
    throw new Error("template-node-type");
}

function sortedAssets(assets: GenerationTemplateAsset[]): GenerationTemplateAsset[] {
    return [...assets].sort((a, b) => (a.sort || 0) - (b.sort || 0));
}

function compiledAsset(asset: GenerationTemplateAsset, role: GenerationTemplateAssetRole): CompiledAsset {
    return { role, url: asset.url.trim(), optional: Boolean(asset.optional) };
}

export function legalVideoOperation(input: {
    startFrame?: CompiledAsset;
    endFrame?: CompiledAsset;
    referenceAssets: CompiledAsset[];
}): { operation: GenerationTemplateLegalOperation; referenceAssets: CompiledAsset[]; degraded: boolean; reason?: string } {
    const named = Boolean(input.startFrame || input.endFrame);
    if (named) {
        if (input.referenceAssets.length) {
            return { operation: "image_to_video", referenceAssets: [], degraded: true, reason: "dropped-mixed-refs" };
        }
        return { operation: "image_to_video", referenceAssets: [], degraded: false };
    }
    if (input.referenceAssets.length) {
        return { operation: "reference_to_video", referenceAssets: input.referenceAssets, degraded: false };
    }
    return { operation: "text_to_video", referenceAssets: [], degraded: false };
}

export function compileGenerationTemplate(detail: GenerationTemplateDetail, input: CompileGenerationTemplateInput): CompiledGenerationTemplate {
    const nodeType = targetNodeType(detail.target, input.selectedNodeType);
    const slots = detail.slots || [];
    const slotValues = fillSlotValues(slots, input.slotValues);
    for (const slot of slots) {
        if (slot.kind === "asset" || !slot.required) continue;
        if (!slotValues[slot.id]) throw new Error(`template-slot:${slot.id}`);
    }
    const negotiated = negotiateTemplateModel(detail.modelIntent?.preferred || "", detail.modelIntent?.fallbacks, input.availableModels, input.currentModel);
    const prompt = compilePromptBody(detail.prompt?.body || "", detail.prompt?.adapters, negotiated.model, slotValues).trim();
    if (!prompt) throw new Error("template-prompt");
    const frames: { startFrame?: CompiledAsset; endFrame?: CompiledAsset; referenceAssets: CompiledAsset[] } = { referenceAssets: [] };
    for (const asset of sortedAssets(detail.assets || [])) {
        if (!asset.url?.trim() || asset.optional && !asset.url.trim()) continue;
        if (asset.role === "start_frame") frames.startFrame = compiledAsset(asset, "start_frame");
        else if (asset.role === "end_frame") frames.endFrame = compiledAsset(asset, "end_frame");
        else if (isReferenceAssetRole(asset.role)) frames.referenceAssets.push(compiledAsset(asset, asset.role));
    }
    let operation: GenerationTemplateLegalOperation | undefined;
    let degraded = negotiated.degraded;
    let degradeReason = negotiated.reason;
    let referenceAssets = frames.referenceAssets;
    if (nodeType === CanvasNodeType.Video) {
        const legal = legalVideoOperation(frames);
        operation = legal.operation;
        referenceAssets = legal.referenceAssets;
        if (legal.degraded) {
            degraded = true;
            degradeReason = legal.reason;
        }
    }
    return {
        template: detail,
        nodeType,
        prompt,
        negative: detail.prompt?.negative?.trim() || undefined,
        slotValues,
        model: negotiated.model,
        operation,
        startFrame: frames.startFrame,
        endFrame: frames.endFrame,
        referenceAssets,
        degraded,
        degradeReason,
        applyPolicy: normalizeApplyPolicy(input.applyPolicy || detail.applyPolicy),
        seconds: compiledSeconds(detail.modelIntent?.constraints),
        size: compiledSize(detail.modelIntent?.constraints),
        stamp: {
            id: detail.id,
            slug: detail.slug,
            version: detail.version,
            appliedAt: new Date().toISOString(),
            degraded,
            degradeReason,
            remixOf: detail.remixOfId || undefined,
            promptBody: detail.prompt?.body || "",
            adapters: detail.prompt?.adapters,
            slots,
        },
    };
}

export function rewriteCompiledPrompt(stamp: { promptBody: string; adapters?: Record<string, string>; slots: GenerationTemplateSlot[] }, model: string | undefined, slotValues: Record<string, string>): string {
    return compilePromptBody(stamp.promptBody, stamp.adapters, model, fillSlotValues(stamp.slots, slotValues)).trim();
}

export function generationTemplateUserError(error: unknown): string {
    const code = error instanceof Error ? error.message : "";
    if (code === "template-node-type") return canvasT("videoCanvas.craft.templateNodeType", "当前节点类型和该模板不匹配");
    if (code === "template-prompt") return canvasT("videoCanvas.craft.templatePromptEmpty", "模板提示词为空");
    if (code.startsWith("template-slot:")) return canvasT("videoCanvas.craft.templateSlotRequired", "请先填完模板必填槽位");
    if (error instanceof Error && error.message.trim()) return error.message;
    return canvasT("videoCanvas.craft.templateApplyFailed", "套用模板失败");
}

export function templateMatchesNode(target: GenerationTemplateTarget, nodeType: CanvasNodeType | string): boolean {
    try {
        return targetNodeType(target, nodeType) === nodeType;
    } catch {
        return false;
    }
}
