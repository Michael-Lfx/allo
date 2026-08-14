/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ProviderModelResponse } from '@/common/types/provider/providerModel';
import type { IProvider } from '@/common/config/storage';

/** Mirrors `FLOWY_CATALOG_CREDIT_RATE_PARAM` in nomifun-db. */
export const FLOWY_CATALOG_CREDIT_RATE_PARAM = '_flowy_catalog_credit_rate';

/** Normalize catalog-owned credit multiplier from `provider_models.params`. */
export function parseCatalogCreditRate(params: unknown): number | undefined {
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return undefined;
  }
  const raw = (params as Record<string, unknown>)[FLOWY_CATALOG_CREDIT_RATE_PARAM];
  if (typeof raw !== 'number' || !Number.isFinite(raw) || raw <= 0) {
    return undefined;
  }
  return raw;
}

export function catalogCreditRateForModel(
  provider: Pick<IProvider, 'models_detail'> | null | undefined,
  modelName: string | undefined
): number | undefined {
  if (!provider?.models_detail?.length || !modelName) {
    return undefined;
  }
  const row = provider.models_detail.find((detail) => detail.model === modelName) as
    | ProviderModelResponse
    | undefined;
  return parseCatalogCreditRate(row?.params);
}

/**
 * Compact multiplier label for dropdown rows, e.g. `×1` / `×0.5` / `×1.25`.
 * Returns `undefined` when the rate should not be shown.
 */
export function formatCreditRateMultiplier(rate: number | undefined | null): string | undefined {
  if (rate == null || !Number.isFinite(rate) || rate <= 0) {
    return undefined;
  }
  // Keep up to 3 decimals, then drop trailing zeros (`1.000` → `1`).
  return `×${Number(rate.toFixed(3))}`;
}
