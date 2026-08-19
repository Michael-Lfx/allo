/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ProviderModelResponse } from '@/common/types/provider/providerModel';
import type { IProvider } from '@/common/config/storage';

/** Matches `CompactConfig` default `context_window`. */
export const DEFAULT_CONTEXT_WINDOW_TOKENS = 128_000;

function positiveLimit(value: number | null | undefined): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : undefined;
}

/**
 * Per-model context window from the live provider catalog.
 * Prefers `models_detail.context_limit`, then `model_context_limits[model]`.
 */
export function catalogContextLimitForModel(
  provider: Pick<IProvider, 'models_detail' | 'model_context_limits'> | null | undefined,
  modelName: string | undefined
): number | undefined {
  if (!modelName) {
    return undefined;
  }
  const row = provider?.models_detail?.find((detail) => detail.model === modelName) as
    | ProviderModelResponse
    | undefined;
  return positiveLimit(row?.context_limit) ?? positiveLimit(provider?.model_context_limits?.[modelName]);
}

/** Ring denominator: catalog window when known, otherwise the engine 128k default. */
export function resolveDisplayContextWindow(
  catalogLimit: number | undefined,
  fallback = DEFAULT_CONTEXT_WINDOW_TOKENS
): number {
  return positiveLimit(catalogLimit) ?? fallback;
}
