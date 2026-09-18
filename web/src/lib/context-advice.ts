/**
 * W9（R14）—— 上下文接近上限时的建议（纯模块）。
 *
 * 口径：`context.usage` / `conversation/messages` 的 `context_usage` 是**服务端测量**值，
 * 客户端只做换算与分级，不猜窗口大小。`percent` 缺失时用 `used/window` 现算，
 * 窗口为 0（引擎未上报）时返回 `null`——不给建议，也不显示 0%。
 *
 * 关于「压缩」：协议里**没有**压缩动作（`11` §5.3 未做），所以建议只指向
 * 「新建会话」这一件真能做的事，不摆一个点了没用的「压缩」按钮。
 */

/** 达到该百分比即提示（与服务端无关，纯展示阈值）。 */
export const CONTEXT_NEAR_LIMIT_PERCENT = 80;

export interface ContextAdviceInput {
  used_tokens: number;
  window_tokens: number;
  percent?: number | null;
}

export interface ContextAdvice {
  level: "ok" | "near";
  /** 0–100 的整数百分比。 */
  percent: number;
}

export function contextAdvice(
  usage: ContextAdviceInput | null | undefined,
  threshold = CONTEXT_NEAR_LIMIT_PERCENT,
): ContextAdvice | null {
  if (!usage) return null;
  if (!Number.isFinite(usage.window_tokens) || usage.window_tokens <= 0) return null;
  const raw = typeof usage.percent === "number" && Number.isFinite(usage.percent)
    ? usage.percent
    : (usage.used_tokens / usage.window_tokens) * 100;
  const percent = Math.max(0, Math.min(100, Math.round(raw)));
  return { level: percent >= threshold ? "near" : "ok", percent };
}
