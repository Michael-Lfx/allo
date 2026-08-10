/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ProviderModelResponse } from '@/common/types/provider/providerModel';
import type { IProvider, TProviderWithModel } from '@/common/config/storage';

/** Mirrors `FLOWY_CATALOG_REASONING_EFFORT_PARAM` in nomifun-db. */
export const FLOWY_CATALOG_REASONING_EFFORT_PARAM = '_flowy_catalog_reasoning_effort';

/** Prefer medium when the catalog lists it; otherwise the first advertised level. */
export function defaultReasoningEffort(levels: readonly string[]): string | undefined {
  if (levels.length === 0) return undefined;
  return levels.find((level) => level === 'medium') ?? levels[0];
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
    const trimmed = entry.trim();
    if (!trimmed || levels.includes(trimmed)) continue;
    levels.push(trimmed);
  }
  return levels;
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
  const levels = catalogReasoningEffortForModel(selection, selection?.use_model);
  if (levels.length === 0) {
    return { levels };
  }
  const preferredTrimmed = preferred?.trim();
  if (preferredTrimmed && levels.includes(preferredTrimmed)) {
    return { levels, effort: preferredTrimmed };
  }
  return { levels, effort: defaultReasoningEffort(levels) };
}
