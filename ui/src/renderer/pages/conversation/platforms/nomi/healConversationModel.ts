/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import type { ConfigKeyMap } from '@/common/config/configKeys';

type SavedDefault = ConfigKeyMap['nomi.defaultModel'];

export type HealModelResult = {
  provider: IProvider;
  use_model: string;
  /** `default` = conversation never bound a model; `stale` = previous binding invalid */
  reason: 'default' | 'stale';
};

/**
 * Resolve a conversation model when none is bound or the binding is no longer
 * available. A conversation can be healed only to the persisted default; a
 * missing or stale default is a deliberate user-selection boundary. Returns
 * null when no heal/default is needed or the saved default is unavailable.
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
    const dp = providers.find((p) => p.id === savedDefault.provider_id);
    if (dp && getAvailableModels(dp).includes(savedDefault.model)) {
      return { provider: dp, use_model: savedDefault.model, reason };
    }
  }
  return null;
}
