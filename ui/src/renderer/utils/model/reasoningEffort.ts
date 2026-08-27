/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ProviderModelResponse } from '@/common/types/provider/providerModel';
import type { IProvider, TProviderWithModel } from '@/common/config/storage';

/** Mirrors `FLOWY_CATALOG_REASONING_EFFORT_PARAM` in nomifun-db. */
export const FLOWY_CATALOG_REASONING_EFFORT_PARAM = '_flowy_catalog_reasoning_effort';

/**
 * Clean catalog-owned effort levels without imposing a client-side ranking.
 * The Cloud array is an ordered contract: its first item is the shallowest
 * advertised level and its last item is the deepest one.
 */
export function normalizeReasoningEffortLevels(levels: readonly string[]): string[] {
  const normalized: string[] = [];
  for (const level of levels) {
    const trimmed = level.trim();
    if (!trimmed || normalized.includes(trimmed)) continue;
    normalized.push(trimmed);
  }
  return normalized;
}

/** Prefer medium when the catalog lists it; otherwise the first advertised level. */
export function defaultReasoningEffort(levels: readonly string[]): string | undefined {
  const normalized = normalizeReasoningEffortLevels(levels);
  if (normalized.length === 0) return undefined;
  return normalized.find((level) => level === 'medium') ?? normalized[0];
}

/** Normalize catalog-owned effort levels from `provider_models.params`. */
export function parseCatalogReasoningEffortLevels(params: unknown): string[] {
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return [];
  }
  const raw = (params as Record<string, unknown>)[FLOWY_CATALOG_REASONING_EFFORT_PARAM];
  if (!Array.isArray(raw)) {
    return [];
  }
  const levels: string[] = [];
  for (const entry of raw) {
    if (typeof entry !== 'string') continue;
    levels.push(entry);
  }
  return normalizeReasoningEffortLevels(levels);
}

export interface ReasoningEffortSliderViewModel {
  levels: string[];
  effort?: string;
  index: number;
  progress: number;
  isStatic: boolean;
}

/** Resolve a possibly stale stored effort to one legal catalog index. */
export function reasoningEffortIndex(levels: readonly string[], effort?: string | null): number {
  const normalized = normalizeReasoningEffortLevels(levels);
  if (normalized.length === 0) return -1;
  const preferred = effort?.trim();
  const selected = preferred && normalized.includes(preferred) ? preferred : defaultReasoningEffort(normalized);
  return Math.max(0, normalized.indexOf(selected ?? normalized[0]));
}

/** Resolve a stored effort against the levels advertised by the active model. */
export function resolveReasoningEffortForLevels(
  levels: readonly string[],
  preferred?: string | null
): { levels: string[]; effort?: string } {
  const normalized = normalizeReasoningEffortLevels(levels);
  if (normalized.length === 0) return { levels: normalized };
  const preferredTrimmed = preferred?.trim();
  return {
    levels: normalized,
    effort:
      preferredTrimmed && normalized.includes(preferredTrimmed)
        ? preferredTrimmed
        : defaultReasoningEffort(normalized),
  };
}

/** Map a discrete slider index back to the server-advertised effort string. */
export function reasoningEffortAtIndex(levels: readonly string[], index: number): string | undefined {
  const normalized = normalizeReasoningEffortLevels(levels);
  if (normalized.length === 0) return undefined;
  const clamped = Math.min(normalized.length - 1, Math.max(0, Math.round(index)));
  return normalized[clamped];
}

/** Return a stable 0..1 fill ratio, with a single level rendered as full/static. */
export function reasoningEffortProgress(index: number, levelCount: number): number {
  if (levelCount <= 0) return 0;
  if (levelCount === 1) return 1;
  return Math.min(1, Math.max(0, index / (levelCount - 1)));
}

/**
 * Keep the latest user choice while another persistence request is in flight.
 *
 * A choice equal to the last confirmed value is still meaningful during an
 * in-flight request: it is the user's request to compensate for that request
 * if the earlier value wins the race. Returning the index makes this small
 * rule easy to exercise without mounting the Arco control.
 */
export function pendingReasoningEffortCommitIndex(
  nextIndex: number,
  confirmedIndex: number,
  isSaving: boolean
): number | null {
  return isSaving || nextIndex !== confirmedIndex ? nextIndex : null;
}

/** Pure view model used by both Guid and Nomi slider surfaces. */
export function reasoningEffortSliderViewModel(
  levels: readonly string[],
  effort?: string | null
): ReasoningEffortSliderViewModel {
  const normalized = normalizeReasoningEffortLevels(levels);
  const index = reasoningEffortIndex(normalized, effort);
  return {
    levels: normalized,
    effort: reasoningEffortAtIndex(normalized, index),
    index,
    progress: reasoningEffortProgress(index, normalized.length),
    isStatic: normalized.length <= 1,
  };
}

export function catalogReasoningEffortForModel(
  provider: Pick<IProvider, 'models_detail'> | null | undefined,
  modelName: string | undefined
): string[] {
  if (!provider?.models_detail?.length || !modelName) {
    return [];
  }
  const row = provider.models_detail.find((detail) => detail.model === modelName) as
    | ProviderModelResponse
    | undefined;
  return parseCatalogReasoningEffortLevels(row?.params);
}

export function resolveReasoningEffortForSelection(
  selection: TProviderWithModel | undefined,
  preferred?: string | null
): { levels: string[]; effort?: string } {
  return resolveReasoningEffortForLevels(
    catalogReasoningEffortForModel(selection, selection?.use_model),
    preferred
  );
}
