import { canvasT } from "@oc/lib/canvas/canvas-i18n";

export const CONTENT_MODERATION_ERROR_CODE = "sensitive_words_detected";

export const CONTENT_MODERATION_MESSAGE = "内容审核未通过，本次平台积分未扣除或已退还。请修改提示词后重新生成。";
export const COPYRIGHT_RESTRICTION_MESSAGE = "提示词可能涉及版权受限内容，内容审核未通过。请修改提示词后重新生成。";
export const REFERENCE_IMAGE_MODERATION_MESSAGE = "参考图未通过内容审核（可能含真人肖像等）。请更换参考图或调整提示词后重试。";
export const REF_AUDIO_DURATION_MESSAGE = "参考音频总时长不能超过 15 秒。请缩短或减少角色参考音后重试。";
export const REF_AUDIO_TOO_SHORT_MESSAGE = "每段参考音频不能短于 1.8 秒。系统会在提交时加长副本；请重试。";
export const AUDIO_UNSUPPORTED_MESSAGE = "当前视频模型不支持参考音频。请改用 Seedance 或 Wan 3.0，或断开音频节点。";
export const AUDIO_NEEDS_VISUAL_MESSAGE = "参考音频需要同时连接至少一张参考图或参考视频。Seedance / Wan 不能只凭音频生成。";
export const FRAME_NOT_IMAGE_MESSAGE = "首帧/尾帧必须是图片（PNG/JPEG/WebP），不能使用音频或视频。";
export const IMAGE_NOT_DECODABLE_MESSAGE = "这张图不是可用的 PNG/JPEG/WebP。请重新导出或换一张图。";
export const AUDIO_AS_IMAGE_MESSAGE = "不能把音频文件当作参考图。请把 WAV 连到音频节点后再生成。";

const DEFAULT_GENERATION_ERROR_MESSAGE = "生成失败，请稍后重试。";
const NETWORK_ERROR_MESSAGE = "网络异常。";

// errorDetails 是持久化状态并参与重试门控（isContentModerationError 精确匹配正典串），
// 因此写入侧必须保持中文正典原文；仅渲染 / Toast 边界经此映射为当前语言。
const GENERATION_ERROR_DISPLAY_KEYS: ReadonlyArray<readonly [string, string]> = [
    ["videoCanvas.genError.moderation", CONTENT_MODERATION_MESSAGE],
    ["videoCanvas.genError.copyright", COPYRIGHT_RESTRICTION_MESSAGE],
    ["videoCanvas.genError.referenceModeration", REFERENCE_IMAGE_MODERATION_MESSAGE],
    ["videoCanvas.genError.refAudioDuration", REF_AUDIO_DURATION_MESSAGE],
    ["videoCanvas.genError.refAudioTooShort", REF_AUDIO_TOO_SHORT_MESSAGE],
    ["videoCanvas.genError.audioUnsupported", AUDIO_UNSUPPORTED_MESSAGE],
    ["videoCanvas.genError.audioNeedsVisual", AUDIO_NEEDS_VISUAL_MESSAGE],
    ["videoCanvas.genError.frameNotImage", FRAME_NOT_IMAGE_MESSAGE],
    ["videoCanvas.genError.imageNotDecodable", IMAGE_NOT_DECODABLE_MESSAGE],
    ["videoCanvas.genError.audioAsImage", AUDIO_AS_IMAGE_MESSAGE],
    ["videoCanvas.genError.network", NETWORK_ERROR_MESSAGE],
    ["videoCanvas.genError.busy", "服务当前繁忙，请稍后重试。"],
    ["videoCanvas.genError.authFailed", "生成服务鉴权失败，请检查渠道配置。"],
    ["videoCanvas.genError.urlUnavailable", "生成服务地址不可用，请检查渠道配置。"],
    ["videoCanvas.genError.failed", DEFAULT_GENERATION_ERROR_MESSAGE],
];

export function localizeGenerationErrorText(text: string | undefined | null): string {
    const raw = text ?? "";
    if (!raw) return "";
    for (const [key, canonical] of GENERATION_ERROR_DISPLAY_KEYS) {
        if (raw === canonical) return canvasT(key, canonical);
    }
    return raw;
}

export type GenerationFailureMetadata = {
    errorDetails: string;
    generationErrorCode?: string;
    failedPromptFingerprint?: string;
};

export function generationFailureMetadata(error: unknown, prompt: string): GenerationFailureMetadata {
    const raw = rawGenerationError(error);
    if (!isContentModerationError(raw)) return { errorDetails: generationErrorMessage(error) };
    return {
        errorDetails: contentModerationMessage(raw),
        generationErrorCode: CONTENT_MODERATION_ERROR_CODE,
        failedPromptFingerprint: generationPromptFingerprint(prompt),
    };
}

export function generationErrorMessage(error: unknown) {
    const raw = rawGenerationError(error);
    const unwrapped = unwrapGenerationErrorLayers(raw);
    if (isContentModerationError(raw) || isContentModerationError(unwrapped)) {
        return contentModerationMessage(isContentModerationError(raw) ? raw : unwrapped);
    }

    const providerMessage = extractStructuredProviderMessage(unwrapped) || extractWrappedProviderMessage(unwrapped) || extractStructuredProviderMessage(raw) || extractWrappedProviderMessage(raw);
    const displayMessage = providerMessage || unwrapped || raw;
    if (isContentModerationError(displayMessage)) return contentModerationMessage(displayMessage);
    if (isRefAudioClipTooShortError(displayMessage) || isRefAudioClipTooShortError(unwrapped) || isRefAudioClipTooShortError(raw)) {
        return REF_AUDIO_TOO_SHORT_MESSAGE;
    }
    if (isRefAudioDurationError(displayMessage) || isRefAudioDurationError(unwrapped) || isRefAudioDurationError(raw)) {
        return REF_AUDIO_DURATION_MESSAGE;
    }
    if (isAudioAsImageError(displayMessage) || isAudioAsImageError(unwrapped) || isAudioAsImageError(raw)) {
        return AUDIO_AS_IMAGE_MESSAGE;
    }
    if (isImageNotDecodableError(displayMessage) || isImageNotDecodableError(unwrapped) || isImageNotDecodableError(raw)) {
        return IMAGE_NOT_DECODABLE_MESSAGE;
    }
    if (isAudioUnsupportedError(displayMessage) || isAudioUnsupportedError(unwrapped)) {
        return AUDIO_UNSUPPORTED_MESSAGE;
    }
    if (isAudioNeedsVisualError(displayMessage) || isAudioNeedsVisualError(unwrapped)) {
        return AUDIO_NEEDS_VISUAL_MESSAGE;
    }
    if (isFrameNotImageError(displayMessage) || isFrameNotImageError(unwrapped)) {
        return FRAME_NOT_IMAGE_MESSAGE;
    }
    if (isNetworkFailure(displayMessage) || isNetworkFailure(raw)) return NETWORK_ERROR_MESSAGE;
    if (hasHttpStatus(raw, 429) || hasHttpStatus(unwrapped, 429)) return "服务当前繁忙，请稍后重试。";
    if (hasHttpStatus(raw, 401, 403) || hasHttpStatus(unwrapped, 401, 403)) return "生成服务鉴权失败，请检查渠道配置。";
    if (hasHttpStatus(raw, 404) || hasHttpStatus(unwrapped, 404)) return "生成服务地址不可用，请检查渠道配置。";
    // Only true transport / bare gateway failures are "网络异常". Provider 5xx with a remaining sentence stays visible.
    if (isBareInfrastructureFailure(displayMessage) && !looksLikeProviderBusinessRejection(displayMessage) && !looksLikeProviderBusinessRejection(unwrapped)) {
        return NETWORK_ERROR_MESSAGE;
    }
    return displayMessage || DEFAULT_GENERATION_ERROR_MESSAGE;
}

export function logCanvasGenerationFailure(scope: string, error: unknown) {
    console.error(`[canvas] ${scope}`, rawGenerationError(error), error);
}

export function isContentModerationError(value: unknown) {
    const text = value instanceof Error ? value.message : String(value || "");
    if (!text.trim()) return false;
    const lower = text.toLowerCase();
    if (lower.includes(CONTENT_MODERATION_ERROR_CODE)) return true;
    if (text.includes("内容审核未通过") || text.includes("版权受限") || text.includes("参考图未通过内容审核")) return true;
    return isProviderContentPolicyRejection(lower);
}

export function unchangedModeratedPrompt(metadata: { errorDetails?: string; generationErrorCode?: string; failedPromptFingerprint?: string } | undefined, prompt: string) {
    const moderationFailure = metadata?.generationErrorCode === CONTENT_MODERATION_ERROR_CODE || isContentModerationError(metadata?.errorDetails);
    if (!moderationFailure) return false;
    if (!metadata?.failedPromptFingerprint) return true;
    return metadata.failedPromptFingerprint === generationPromptFingerprint(prompt);
}

// 指纹只用于识别“原样重试”，不是安全或鉴权用途。
export function generationPromptFingerprint(value: string) {
    const normalized = value.trim().replace(/\s+/g, " ");
    let hash = 2166136261;
    for (let index = 0; index < normalized.length; index += 1) {
        hash ^= normalized.charCodeAt(index);
        hash = Math.imul(hash, 16777619);
    }
    return `${normalized.length}:${(hash >>> 0).toString(36)}`;
}

function contentModerationMessage(raw: string) {
    const lower = raw.toLowerCase();
    if (isCopyrightRestriction(lower) || raw.includes("版权受限")) return COPYRIGHT_RESTRICTION_MESSAGE;
    if (isReferenceImageModeration(lower) || raw.includes("参考图未通过内容审核")) return REFERENCE_IMAGE_MODERATION_MESSAGE;
    return CONTENT_MODERATION_MESSAGE;
}

function isRefAudioClipTooShortError(raw: string) {
    const lower = raw.toLowerCase();
    const mentionsAudio = lower.includes("audio duration") || lower.includes("reference_audio");
    const mentionsFloor =
        lower.includes("greater than or equal")
        || lower.includes("must be greater")
        || lower.includes("at least");
    return mentionsAudio && mentionsFloor && (lower.includes("1.8") || lower.includes("1.80"));
}

function isRefAudioDurationError(raw: string) {
    const lower = raw.toLowerCase();
    return (
        lower.includes("reference_audio") &&
        (lower.includes("exceeds max") || (lower.includes("total duration") && lower.includes("15")))
    );
}

function isAudioAsImageError(raw: string) {
    const lower = raw.toLowerCase();
    return (
        (lower.includes("cannot be used as an image") && (lower.includes("audio") || lower.includes("wav")))
        || raw.includes("不能把音频文件当作参考图")
    );
}

function isImageNotDecodableError(raw: string) {
    return /not a decodable png\/?jpe?g\/?webp/i.test(raw) || raw.includes("不是可用的 PNG/JPEG/WebP");
}

function isAudioUnsupportedError(raw: string) {
    return raw.includes("不支持参考音频") || /does not support reference audio/i.test(raw);
}

function isAudioNeedsVisualError(raw: string) {
    return raw.includes("不能只凭音频生成") || /cannot be the only reference/i.test(raw);
}

function isFrameNotImageError(raw: string) {
    return raw.includes("首帧/尾帧必须是图片") || /first\/last frame must be an image/i.test(raw);
}

/** 与后端 nomi-vimax / nomifun-cloud 的敏感内容判定对齐。 */
function isProviderContentPolicyRejection(lower: string) {
    return (
        lower.includes("sensitivecontent")
        || lower.includes("inputtextsensitive")
        || lower.includes("inputimagesensitive")
        || lower.includes("sensitive content")
        || lower.includes("inappropriate content")
        || lower.includes("datainspectionfailed")
        || lower.includes("policyviolation")
        || lower.includes("privacyinformation")
        || lower.includes("may contain real person")
        || lower.includes("copyright restriction")
        || lower.includes("related to copyright")
        || lower.includes("内容安全")
        || lower.includes("敏感内容")
        || lower.includes("不当内容")
        || lower.includes("含真人")
    );
}

function isCopyrightRestriction(lower: string) {
    return lower.includes("copyright") || lower.includes("版权受限") || lower.includes("related to copyright");
}

function isReferenceImageModeration(lower: string) {
    return (
        lower.includes("inputimagesensitive")
        || lower.includes("privacyinformation")
        || lower.includes("may contain real person")
        || (lower.includes("real person") && lower.includes("sensitive"))
        || lower.includes("含真人")
    );
}

function looksLikeProviderBusinessRejection(raw: string) {
    const lower = raw.toLowerCase();
    return (
        isProviderContentPolicyRejection(lower)
        || lower.includes("invalidparameter")
        || lower.includes("invalid parameter")
        || /\binvalid\b/.test(lower)
        || lower.includes("last_frame")
        || lower.includes("first_frame")
        || lower.includes("model call failed")
        || lower.includes("insufficient_credit")
        || lower.includes("insufficient credit")
        || lower.includes("credit balance is too low")
        || lower.includes("积分不足")
        || lower.includes("余额不足")
        || lower.includes("额度")
        || (lower.includes("reference_audio") && (lower.includes("exceeds max") || lower.includes("total duration")))
        || lower.includes("not a decodable png")
        || lower.includes("cannot be used as an image")
        || lower.includes("首帧/尾帧必须是图片")
        || lower.includes("不支持参考音频")
        || lower.includes("不能只凭音频生成")
    );
}

export function rawGenerationError(error: unknown) {
    if (error instanceof Error) return error.message.trim();
    if (typeof error === "string") return error.trim();
    return providerPayloadMessage(error);
}

function unwrapGenerationErrorLayers(raw: string) {
    let text = raw.trim();
    if (!text) return "";
    text = text.replace(/^Internal error:\s*/i, "");
    text = text.replace(/^Bad request:\s*/i, "");
    text = text.replace(/^(?:video|image) generation failed:\s*/i, "");
    const cause = text.match(/(?:^|\n)Cause:\s*(.+?)(?:\nHint:|\nRequest id:|$)/is);
    if (cause?.[1]) text = cause[1].trim();
    text = text.replace(/^API error \d{3}:\s*/i, "");
    text = text.replace(/\s*Request id:\s*\S+[\s\S]*$/i, "");
    text = text.replace(/\s*Hint:\s[\s\S]*$/i, "");
    return text.trim();
}

function isBareInfrastructureFailure(value: string) {
    const text = value.trim();
    if (!text) return true;
    if (isNetworkFailure(text)) return true;
    if (/<!DOCTYPE|<\/html>/i.test(text)) return true;
    if (/^(?:Bad Gateway|Service Unavailable|Gateway Timeout|Internal Server Error)$/i.test(text)) return true;
    if (/^Request failed with status code (?:502|503|504)\b/i.test(text)) return true;
    if (containsInfrastructureDetails(text) && text.length < 96 && !looksLikeProviderBusinessRejection(text)) return true;
    return false;
}

function extractStructuredProviderMessage(raw: string) {
    for (let index = raw.indexOf("{"); index >= 0; index = raw.indexOf("{", index + 1)) {
        try {
            const message = providerPayloadMessage(JSON.parse(raw.slice(index).trim()));
            if (message) return message;
        } catch {
            // 上游常把 JSON 拼在 HTTP 状态后；不是完整 JSON 时继续尝试下一个对象起点。
        }
    }
    return "";
}

function extractWrappedProviderMessage(raw: string) {
    const interfaceFailure = raw.match(/^接口请求失败[:：]\s*(.*)$/s);
    const requestFailure = raw.match(/^Request failed with status code \d{3}\s*[:：-]?\s*(.+)$/is);
    const wrapped = interfaceFailure?.[1] ?? requestFailure?.[1];
    if (!wrapped) return "";
    const message = wrapped
        .replace(/^\d{3}(?:\s+(?:Bad Gateway|Service Unavailable|Gateway Timeout|Internal Server Error|Not Found|Unauthorized|Forbidden|Too Many Requests))?\s*[:：-]?\s*/i, "")
        .trim();
    return message && !containsInfrastructureDetails(message) ? message : "";
}

function providerPayloadMessage(payload: unknown): string {
    if (typeof payload === "string") return payload.trim();
    if (!payload || typeof payload !== "object" || Array.isArray(payload)) return "";
    const record = payload as Record<string, unknown>;
    if (record.error && typeof record.error === "object") {
        const nested = providerPayloadMessage(record.error);
        if (nested) return nested;
    }
    for (const key of ["message", "msg", "detail"] as const) {
        const value = record[key];
        if (typeof value === "string" && value.trim()) return value.trim();
    }
    return typeof record.error === "string" ? record.error.trim() : "";
}

function isNetworkFailure(value: string) {
    return /\b(?:dial tcp|connection refused|connection reset|no such host|i\/o timeout|context deadline exceeded|network error|failed to fetch|fetch failed|socket hang up|econnrefused|econnreset|etimedout)\b/i.test(value);
}

function hasHttpStatus(value: string, ...statuses: number[]) {
    return statuses.some((status) => new RegExp(`\\b${status}\\b`).test(value));
}

function containsInfrastructureDetails(value: string) {
    return /(?:接口请求失败|Request failed with status code|https?:\/\/|\b(?:GET|POST|PUT|PATCH|DELETE)\s+["']?|Bad Gateway|Service Unavailable|Gateway Timeout|upstream_error)/i.test(value);
}
