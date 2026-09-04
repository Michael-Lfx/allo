import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasLlmNetworkErrorMessage } from "@renderer/pages/videoCanvas/lib/canvasLlm";
import { generationErrorMessage, localizeGenerationErrorText } from "@oc/lib/generation-error";

const GENERIC_FALLBACK = "操作失败，请稍后重试。";
const VISION_MODEL_MISSING = "未配置支持图片理解的文本模型（模型 extra.input 需包含 image）";
const GENERATION_CANONICAL = new Set([
  "内容审核未通过，本次平台积分未扣除或已退还。请修改提示词后重新生成。",
  "提示词可能涉及版权受限内容，内容审核未通过。请修改提示词后重新生成。",
  "参考图未通过内容审核（可能含真人肖像等）。请更换参考图或调整提示词后重试。",
  "网络异常。",
  "服务当前繁忙，请稍后重试。",
  "生成服务鉴权失败，请检查渠道配置。",
  "生成服务地址不可用，请检查渠道配置。",
  "生成失败，请稍后重试。",
]);

const CRITIQUE_PARSE_CODES = new Set([
  "art_critique_candidates_invalid",
  "art_critique_scene_invalid",
  "art_critique_aggregate_invalid",
  "art_critique_grounding_invalid",
  "art_critique_verification_invalid",
  "art_critique_edit_prompt_invalid",
  "art_critique_result_invalid",
  "art_critique_result_empty",
  "art_critique_result_missing",
]);

function rawErrorText(error: unknown): string {
  if (error instanceof Error) return error.message.trim();
  if (typeof error === "string") return error.trim();
  if (error == null) return "";
  try {
    const serialized = JSON.stringify(error);
    return serialized === "{}" ? "" : serialized;
  } catch {
    return String(error);
  }
}

function isAbortError(error: unknown, raw: string) {
  if (error instanceof DOMException && error.name === "AbortError") return true;
  if (error instanceof Error && error.name === "AbortError") return true;
  return raw === "请求已取消" || raw === "art_critique_aborted" || /aborted|aborterror/i.test(raw);
}

function looksLikeInternalErrorCode(text: string) {
  return /^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$/i.test(text.trim());
}

function looksLikeRawInfrastructure(text: string) {
  return (
    /stack trace|traceback|at \w+\s*\(|ECONNREFUSED|ETIMEDOUT|<!DOCTYPE|upstream_error/i.test(text)
    || (text.trim().startsWith("{") && /"(?:error|message|code)"/.test(text))
  );
}

function mapKnownCanvasError(raw: string): string | null {
  if (raw === VISION_MODEL_MISSING || /未配置支持图片理解/.test(raw) || /extra\.input 需包含 image/.test(raw)) {
    return canvasT("videoCanvas.userError.visionModelMissing", "当前文本模型不支持理解图片。请到设置中选择支持图片理解的模型后再试。");
  }
  if (raw === "art_critique_pipeline_failed") {
    return canvasT("videoCanvas.userError.critiqueFailed", "画面分析未完成，请稍后重试。若持续失败，请更换支持图片理解的文本模型。");
  }
  if (CRITIQUE_PARSE_CODES.has(raw)) {
    return canvasT("videoCanvas.userError.critiqueParse", "模型返回的分析结果不完整，请重试。");
  }
  if (/_missing$|_invalid_json$/.test(raw)) {
    return canvasT("videoCanvas.userError.critiqueMissing", "模型没有按约定返回分析结果，请稍后重试或更换文本模型。");
  }
  if (raw === canvasLlmNetworkErrorMessage() || /规划模型请求失败/.test(raw)) {
    return canvasT("videoCanvas.userError.llmFailed", "文本模型请求失败，请确认网络和模型配置后重试。");
  }
  if (/tool[_\s-]?choice|thinking\s+mode|parallel_tool_calls|does not support.*tool/i.test(raw)) {
    return canvasT("videoCanvas.userError.toolChoice", "当前文本模型不支持这种调用方式，请更换文本模型后重试。");
  }
  if (/\b429\b|too many requests|rate limit/i.test(raw)) {
    return canvasT("videoCanvas.userError.busy", "服务当前繁忙，请稍后重试。");
  }
  if (/\b(?:401|403)\b|unauthorized|forbidden|鉴权失败/i.test(raw) && /http|status|auth|token|登录/i.test(raw)) {
    return canvasT("videoCanvas.userError.auth", "登录已失效或没有权限，请重新登录后重试。");
  }
  return null;
}

/**
 * Map any canvas-facing failure to a short, user-readable sentence.
 * Never show snake_case codes, HTTP dumps, or stack traces in the UI.
 */
export function formatCanvasUserError(error: unknown, fallback?: string): string {
  const generic = canvasT("videoCanvas.userError.generic", fallback || GENERIC_FALLBACK);
  const raw = rawErrorText(error);
  if (isAbortError(error, raw)) return canvasT("videoCanvas.userError.cancelled", "已取消");
  if (!raw) return generic;

  const known = mapKnownCanvasError(raw);
  if (known) return known;

  const generation = generationErrorMessage(error);
  const localizedGeneration = localizeGenerationErrorText(generation);
  if (GENERATION_CANONICAL.has(generation) && generation !== "生成失败，请稍后重试。") {
    return localizedGeneration;
  }

  if (looksLikeInternalErrorCode(raw) || looksLikeRawInfrastructure(raw)) return generic;

  if (localizedGeneration && !looksLikeInternalErrorCode(localizedGeneration) && !looksLikeRawInfrastructure(localizedGeneration)) {
    const text = localizedGeneration.length > 280 ? `${localizedGeneration.slice(0, 280)}…` : localizedGeneration;
    if (text && text !== "生成失败，请稍后重试。") return text;
  }

  if (looksLikeInternalErrorCode(raw)) return generic;
  if (raw.length > 280) return generic;
  return raw || generic;
}
