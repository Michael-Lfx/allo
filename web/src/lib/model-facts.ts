/**
 * W9（R14）—— 模型目录事实的展示与判定（纯模块）。
 *
 * 数据来自 `conversation/model-options` 里 models.dev 目录的投影，字段**可能整组缺席**
 * （provider 未映射 / 模型不在目录里）。这里的每条函数都遵守同一口径：**不知道就返回
 * `null` / 空数组，绝不把「未知」渲染成 `0` 或「不支持」**。
 */

import type { ConversationModelOptions } from "./protocol";

export interface ModelFacts {
  contextLimit?: number | null;
  catalogContextWindow?: number | null;
  costInput?: number | null;
  costOutput?: number | null;
  supportsVision?: boolean | null;
}

/** 目录里每百万 token 的价格文本（USD）；两个价位都缺就返回 `null`。 */
export function costRateText(facts: ModelFacts): string | null {
  const input = rateText(facts.costInput);
  const output = rateText(facts.costOutput);
  if (input === null && output === null) return null;
  const parts: string[] = [];
  if (input !== null) parts.push(`$${input}/M in`);
  if (output !== null) parts.push(`$${output}/M out`);
  return parts.join(" · ");
}

/** 价格数值：整数不带小数（`3`），小数最多两位并去掉尾零（`0.15`）。 */
export function rateText(value: number | null | undefined): string | null {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return null;
  const rounded = Math.round(value * 100) / 100;
  return Number.isInteger(rounded) ? `${rounded}` : `${rounded}`.replace(/0+$/, "").replace(/\.$/, "");
}

/**
 * 上下文窗口（token）。目录值与配置值都可能有，取**目录优先**、配置兜底；
 * 都没有则 `null`（界面不显示这一段，而不是显示 0k）。
 */
export function contextWindowTokens(facts: ModelFacts): number | null {
  const catalog = facts.catalogContextWindow;
  if (typeof catalog === "number" && catalog > 0) return catalog;
  const configured = facts.contextLimit;
  if (typeof configured === "number" && configured > 0) return configured;
  return null;
}

/**
 * 能力标签。只有**目录明确为 true** 才给标签：`false` 与「未知」都返回空数组——
 * 「目录没说支持图片」不等于「目录说不支持图片」，后者需要用户去别处确认。
 */
export function capabilityLabels(facts: ModelFacts): string[] {
  return facts.supportsVision === true ? ["vision"] : [];
}

/**
 * 发送前兼容性校验（W9）：显式选中的模型是否在当前已知目录里。
 *
 * 返回 i18n key 表示**拦截**，`null` 表示放行。已知目录为空时一律放行——目录没加载
 * 完就拦下发送，是把「我们还没拿到数据」说成「你的模型不存在」。
 */
export function validateModelSelection(input: {
  selectedKey: string | null;
  directoryKeys: readonly string[];
  optionKeys: readonly string[];
}): string | null {
  const key = input.selectedKey?.trim();
  if (!key) return null;
  const known = [...input.directoryKeys, ...input.optionKeys].filter((entry) => entry.length > 0);
  if (known.length === 0) return null;
  return known.includes(key) ? null : "composer.modelUnknown";
}

/** 会话模型与选项里的条目做 `provider/model` 键（目录用 provider_name）。 */
export function modelKey(provider: string, model: string): string {
  return `${provider}/${model}`;
}

// ── W9（R14）：按 turn 的费用 ─────────────────────────────────────────────
//
// 口径（D-W9-1 落地）：逐轮 token 由**运行时**给出（`TurnCompleted` → `message.activity`
// 的 `usage`），费率由 models.dev 目录给出。两者**都在**才算金额；任一缺席整段不显示。
// 绝不拿「上下文占用 × 费率」冒充本轮花费——占用是水位（最后一次请求的提示词），
// 不是本轮账单。

/** 费率数值：0 / NaN / ∞ 一律视为「未知」（与 `rateText` 同一口径）。 */
function rateValue(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

/**
 * 某模型键（`provider/model`）在 `conversation/model-options` 里的目录事实。
 * 键为空、provider 未映射、模型不在目录里——一律 `null`：界面据此不显示这一段，
 * 而不是拿别的模型的价格顶上。
 */
export function modelFactsFor(
  options: ConversationModelOptions | null | undefined,
  key: string | null | undefined,
): ModelFacts | null {
  const trimmed = key?.trim();
  if (!options || !trimmed) return null;
  const index = trimmed.indexOf("/");
  if (index <= 0 || index >= trimmed.length - 1) return null;
  const provider = trimmed.slice(0, index);
  const name = trimmed.slice(index + 1);
  const model = options.providers
    .find((entry) => entry.name === provider)
    ?.models.find((entry) => entry.name === name);
  if (!model) return null;
  return {
    contextLimit: model.context_limit,
    catalogContextWindow: model.catalog_context_window,
    costInput: model.cost_input,
    costOutput: model.cost_output,
    supportsVision: model.supports_vision,
  };
}

/**
 * 本轮用量归属的模型键（`provider/model`）。与模型芯片同一口径：用户显式选择优先、
 * 会话自身记录的模型兜底；两者都拿不到就 `null`——不知道是哪张账单，就不算金额。
 */
export function turnModelKey(
  selectedKey: string | null | undefined,
  conversationModel: { provider_id: string; model: string } | null | undefined,
): string | null {
  const explicit = selectedKey?.trim();
  if (explicit) return explicit;
  const provider = conversationModel?.provider_id?.trim();
  const model = conversationModel?.model?.trim();
  if (!provider || !model) return null;
  return modelKey(provider, model);
}

/**
 * 本轮金额（USD）：**费率与 token 两者都在**才计算，任一缺席返回 `null`。
 *
 * 单向费率乘不出完整账单（只乘输入、把输出当 0 是伪造），token 未知同样算不出——
 * 两种情形都返回 `null`，由调用方整段不渲染。
 */
export function turnCostUsd(
  facts: ModelFacts | null | undefined,
  usage: { input_tokens: number; output_tokens: number } | null | undefined,
): number | null {
  if (!facts || !usage) return null;
  const inputRate = rateValue(facts.costInput);
  const outputRate = rateValue(facts.costOutput);
  if (inputRate === null || outputRate === null) return null;
  const input = usage.input_tokens;
  const output = usage.output_tokens;
  if (!Number.isFinite(input) || !Number.isFinite(output) || input < 0 || output < 0) return null;
  if (input + output <= 0) return null;
  return (input / 1_000_000) * inputRate + (output / 1_000_000) * outputRate;
}

/**
 * 金额文本（USD）：按量级取精度并去掉尾零（`$0.0042` / `$1.25` / `$12`）。
 * 非有限值或非正数返回 `null`——没有金额就不显示，而不是显示 `$0`。
 */
export function costText(usd: number | null | undefined): string | null {
  if (typeof usd !== "number" || !Number.isFinite(usd) || usd <= 0) return null;
  const decimals = usd < 0.0001 ? 6 : usd < 0.01 ? 4 : 2;
  const fixed = usd.toFixed(decimals);
  return `$${fixed.includes(".") ? fixed.replace(/0+$/, "").replace(/\.$/, "") : fixed}`;
}
