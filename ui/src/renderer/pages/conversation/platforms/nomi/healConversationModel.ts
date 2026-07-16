/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';

type SavedDefault = { id: string; use_model: string } | undefined;

export type HealModelResult = {
  provider: IProvider;
  use_model: string;
  /** `default` = conversation never bound a model; `stale` = previous binding invalid */
  reason: 'default' | 'stale';
};

/**
 * Resolve a conversation model when none is bound or the binding is no longer
 * available. Preference order: saved default → first available catalog model.
 * Returns null when no heal/default is needed or nothing is available.
 */
export function resolveHealModel(
  bound: TProviderWithModel | undefined,
  providers: IProvider[],
  getAvailableModels: (p: IProvider) => string[],
  savedDefault: SavedDefault
): HealModelResult | null {
  if (!providers.length) return null;

  const boundProvider = bound?.id ? providers.find((p) => p.id === bound.id) : undefined;
  const boundStillValid =
    !!boundProvider && !!bound?.use_model && getAvailableModels(boundProvider).includes(bound.use_model);
  if (boundStillValid) return null;

  const reason: HealModelResult['reason'] = bound?.id ? 'stale' : 'default';

  if (savedDefault) {
    const dp = providers.find((p) => p.id === savedDefault.id);
    if (dp && getAvailableModels(dp).includes(savedDefault.use_model)) {
      return { provider: dp, use_model: savedDefault.use_model, reason };
    }
  }
  const first = providers[0];
  const firstModel = getAvailableModels(first)[0];
  if (!firstModel) return null;
  return { provider: first, use_model: firstModel, reason };
}
