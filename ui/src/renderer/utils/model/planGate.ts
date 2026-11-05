/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Mirrors the reserved catalog keys in nomifun-db. */
export const FLOWY_CATALOG_MIN_TIER_PARAM = '_flowy_catalog_min_tier';
export const FLOWY_CATALOG_REQUIRED_PLAN_NAME_PARAM = '_flowy_catalog_required_plan_name';
export const FLOWY_CATALOG_REQUIRED_PLAN_CODE_PARAM = '_flowy_catalog_required_plan_code';

export interface CatalogPlanGate {
  minTierLevel: number;
  requiredPlanName?: string;
  requiredPlanCode?: string;
}

function stringParam(params: Record<string, unknown>, key: string): string | undefined {
  const value = params[key];
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

/** Read the catalog plan gate. Tier `0` means the model is open to everyone. */
export function parseCatalogPlanGate(params: unknown): CatalogPlanGate {
  if (!params || typeof params !== 'object' || Array.isArray(params)) {
    return { minTierLevel: 0 };
  }
  const record = params as Record<string, unknown>;
  const raw = record[FLOWY_CATALOG_MIN_TIER_PARAM];
  const minTierLevel = typeof raw === 'number' && Number.isFinite(raw) && raw > 0 ? Math.trunc(raw) : 0;
  return {
    minTierLevel,
    requiredPlanName: stringParam(record, FLOWY_CATALOG_REQUIRED_PLAN_NAME_PARAM),
    requiredPlanCode: stringParam(record, FLOWY_CATALOG_REQUIRED_PLAN_CODE_PARAM),
  };
}

export function isPlanLocked(minTierLevel: number, effectiveTier: number): boolean {
  return minTierLevel > effectiveTier;
}
