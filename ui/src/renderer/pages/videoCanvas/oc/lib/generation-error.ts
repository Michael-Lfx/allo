export const CONTENT_MODERATION_ERROR_CODE = "sensitive_words_detected";

export const CONTENT_MODERATION_MESSAGE = "内容审核未通过，本次平台积分未扣除或已退还。请修改提示词后重新生成。";
export const COPYRIGHT_RESTRICTION_MESSAGE = "提示词可能涉及版权受限内容，内容审核未通过。请修改提示词后重新生成。";
export const REFERENCE_IMAGE_MODERATION_MESSAGE = "参考图未通过内容审核（可能含真人肖像等）。请更换参考图或调整提示词后重试。";

const DEFAULT_GENERATION_ERROR_MESSAGE = "生成失败，请稍后重试。";
const NETWORK_ERROR_MESSAGE = "网络异常。";

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
    if (isContentModerationError(raw)) return contentModerationMessage(raw);

    const providerMessage = extractStructuredProviderMessage(raw) || extractWrappedProviderMessage(raw);
    const displayMessage = providerMessage || raw;
    if (isContentModerationError(displayMessage)) return contentModerationMessage(displayMessage);
    if (isNetworkFailure(displayMessage)) return NETWORK_ERROR_MESSAGE;
    if (!providerMessage) {
        if (hasHttpStatus(raw, 429)) return "服务当前繁忙，请稍后重试。";
        if (hasHttpStatus(raw, 401, 403)) return "生成服务鉴权失败，请检查渠道配置。";
        if (hasHttpStatus(raw, 404)) return "生成服务地址不可用，请检查渠道配置。";
        // 上游常把业务拒绝包成 HTTP 5xx；有明确业务语义时不要误报「网络异常」。
        if ((hasHttpStatus(raw, 500, 502, 503, 504) || containsInfrastructureDetails(raw)) && !looksLikeProviderBusinessRejection(raw)) {
            return NETWORK_ERROR_MESSAGE;
        }
    }
    return displayMessage || DEFAULT_GENERATION_ERROR_MESSAGE;
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
        || lower.includes("insufficient_credit")
        || lower.includes("insufficient credit")
        || lower.includes("credit balance is too low")
        || lower.includes("积分不足")
        || lower.includes("余额不足")
        || lower.includes("额度")
    );
}

function rawGenerationError(error: unknown) {
    if (error instanceof Error) return error.message.trim();
    if (typeof error === "string") return error.trim();
    return providerPayloadMessage(error);
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
