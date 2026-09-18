/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import { resolveModelFallbackIcon } from './modelLogos';

/**
 * Client-side showcase metadata for chat models: brand icon + positioning
 * tagline + recommendation flag.
 *
 * The catalog delivers display names and credit rates only, so taglines live
 * in a hand-maintained registry here (localized through `I18nKey`, which makes
 * a missing translation a compile error). Models the registry does not know
 * degrade gracefully: the row keeps the plain name + credit-rate layout, and
 * the brand icon still resolves through the shared vendor-logo fallback when
 * the name matches a known vendor.
 */
export interface ModelShowcaseEntry {
  taglineKey?: I18nKey;
  /** Pins the model to the top of the Cloud group and shows the badge. */
  recommended?: boolean;
}

export interface ModelShowcase {
  /** Built-in vendor logo URL, or `''` when the brand is unknown. */
  icon: string;
  taglineKey?: I18nKey;
  recommended: boolean;
}

/** Registry keys are normalized ids (lowercase, catalog prefixes stripped). */
export const MODEL_SHOWCASE: Record<string, ModelShowcaseEntry> = {
  'auto-intelligence': { taglineKey: 'conversation.modelPicker.tagline.auto-intelligence' },
  'auto-balance': { taglineKey: 'conversation.modelPicker.tagline.auto-balance' },
  'auto-cost': { taglineKey: 'conversation.modelPicker.tagline.auto-cost' },
  'deepseek-v4-flash': { taglineKey: 'conversation.modelPicker.tagline.deepseek-v4-flash' },
  'deepseek-v4-flash-vision-exp': {
    taglineKey: 'conversation.modelPicker.tagline.deepseek-v4-flash-vision-exp',
  },
  'deepseek-v4.1-flash': {
    taglineKey: 'conversation.modelPicker.tagline.deepseek-v4-1-flash',
    recommended: true,
  },
  'deepseek-v4-pro': { taglineKey: 'conversation.modelPicker.tagline.deepseek-v4-pro' },
  'glm-5': { taglineKey: 'conversation.modelPicker.tagline.glm-5' },
  'glm-5.2': { taglineKey: 'conversation.modelPicker.tagline.glm-5-2' },
  'kimi-k2.5': { taglineKey: 'conversation.modelPicker.tagline.kimi-k2-5' },
  'kimi-k2.6': { taglineKey: 'conversation.modelPicker.tagline.kimi-k2-6' },
  'minimax-m2.7': { taglineKey: 'conversation.modelPicker.tagline.minimax-m2-7' },
  'minimax-m3': { taglineKey: 'conversation.modelPicker.tagline.minimax-m3' },
  'qwen3.7-plus': { taglineKey: 'conversation.modelPicker.tagline.qwen3-7-plus' },
  'qwen3.7-max': { taglineKey: 'conversation.modelPicker.tagline.qwen3-7-max' },
  'qwen3.8-flash': { taglineKey: 'conversation.modelPicker.tagline.qwen3-8-flash' },
};

/** Catalog ids arrive as `AIPC-…` or `flowy/…`; strip prefixes and normalize case. */
export function normalizeShowcaseModelId(model: string): string {
  return model
    .trim()
    .toLowerCase()
    .replace(/^(?:aipc-|flowy\/)+/, '')
    .trim();
}

/**
 * A registered key matches as a prefix only on a delimiter boundary, so a new
 * `glm-50` never inherits `glm-5`'s copy while `glm-5-turbo` does.
 */
const isPrefixBoundary = (id: string, key: string): boolean => {
  const next = id[key.length];
  return next === '-' || next === '.';
};

const findByPrefix = (id: string): ModelShowcaseEntry | undefined => {
  let bestLength = 0;
  let bestEntry: ModelShowcaseEntry | undefined;
  for (const [key, entry] of Object.entries(MODEL_SHOWCASE)) {
    if (key.length >= id.length || key.length <= bestLength) continue;
    if (!id.startsWith(key) || !isPrefixBoundary(id, key)) continue;
    bestLength = key.length;
    bestEntry = entry;
  }
  return bestEntry;
};

/**
 * Resolve the showcase metadata for a catalog model id. Exact registry hits win;
 * unknown ids fall back to the longest registered prefix (variant ids inherit
 * their base model's tagline) and then to the shared vendor-logo fallback.
 * `recommended` never inherits — only the exact registered model is pinned.
 */
export function resolveModelShowcase(model: string): ModelShowcase {
  const id = normalizeShowcaseModelId(model);
  const exact = MODEL_SHOWCASE[id];
  const matched = exact ?? findByPrefix(id);
  return {
    icon: resolveModelFallbackIcon(id),
    taglineKey: matched?.taglineKey,
    recommended: exact?.recommended ?? false,
  };
}

const warnedModels = new Set<string>();

/** Tagline key for an Auto tier (`intelligence` | `balance` | `cost`). */
export function autoTierTaglineKey(tier?: string): I18nKey | undefined {
  return MODEL_SHOWCASE[`auto-${tier || 'balance'}`]?.taglineKey;
}

/** Dev-only, warn-once diagnostic for cloud models missing a showcase entry. */
export function warnMissingShowcaseEntry(model: string): void {
  if (!import.meta.env.DEV || warnedModels.has(model)) return;
  warnedModels.add(model);
  console.warn(
    `[model-showcase] cloud model "${model}" has no registered tagline; ` +
      'the row falls back to the plain name + credit-rate layout.'
  );
}
