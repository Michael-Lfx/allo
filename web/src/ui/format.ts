/**
 * Display formatting shared across the chat shell.
 *
 * These helpers carry UI wording (and, for `modelChipLabel`, a fallback
 * label), so they live outside `src/lib` — that directory stays React-free
 * and wording-free so it can be extracted as `@agent-store/client`
 * (see README "目录结构" and docs/agent-store/07-typescript-sdk.md).
 *
 * Helpers with a single consumer are deliberately kept next to that
 * consumer rather than collected here.
 */

import type {
  ConversationModelOptions,
  ProviderWithModel,
} from "../lib/protocol";

export function shortId(id: string): string {
  return id.length <= 10 ? id : `${id.slice(0, 8)}...`;
}

export function formatTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}k`;
  return `${value}`;
}

/** Strip the free-tier suffix so the chip shows the model the user picked. */
export function modelName(model: string): string {
  return model.replace(/-free$/i, "");
}

export function providerLabel(model: ProviderWithModel): string {
  return modelName(model.model);
}

/** `"provider/model"` → selection object; a bare key is its own provider. */
export function modelKeyToSelection(key: string): ProviderWithModel {
  const index = key.indexOf("/");
  if (index <= 0 || index >= key.length - 1) return { provider_id: key, model: key };
  return { provider_id: key.slice(0, index), model: key.slice(index + 1) };
}

/** Compact relative timestamp (e.g. `7d`, `3h`, `刚刚`): sidebar conversation rows. */
export function formatRelativeTime(value: number, now = Date.now()): string {
  const diff = Math.max(0, now - value);
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (diff < minute) return "刚刚";
  if (diff < hour) return `${Math.floor(diff / minute)}m`;
  if (diff < day) return `${Math.floor(diff / hour)}h`;
  if (diff < 30 * day) return `${Math.floor(diff / day)}d`;
  if (diff < 365 * day) return `${Math.floor(diff / (30 * day))}mo`;
  return `${Math.floor(diff / (365 * day))}y`;
}

export function modelChipLabel(
  current: ProviderWithModel | null,
  selectedKey: string | null,
  options: ConversationModelOptions | null,
  fallbackLabel = "默认模型",
  effortLabel?: string,
): string {
  const base = selectedKey
    ? (() => {
        const [, name] = selectedKey.split("/");
        const display = options?.providers
          .flatMap((entry) => entry.models)
          .find((entry) => entry.name === name)
          ?.display_name;
        return display ?? name ?? selectedKey;
      })()
    : current ? modelName(current.model) : fallbackLabel;
  return effortLabel ? `${base} · ${effortLabel}` : base;
}
