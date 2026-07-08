/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';

/** Strip Flowy catalog `AIPC-` prefix for user-facing model labels. */
export function formatCloudModelLabel(
  modelName: string,
  descriptions?: Record<string, string>
): string {
  const described = descriptions?.[modelName]?.trim();
  const raw = described || modelName;
  return raw.replace(/^AIPC-/i, '');
}

export function findProviderById(providers: IProvider[], providerId?: string): IProvider | undefined {
  if (!providerId) return undefined;
  return providers.find((p) => p.id === providerId);
}

/** Merge persisted conversation model with live provider catalog (descriptions, health, etc.). */
export function hydrateProviderWithModel(
  providers: IProvider[],
  model?: TProviderWithModel | null
): TProviderWithModel | undefined {
  if (!model?.id || !model.use_model) return model ?? undefined;
  const provider = findProviderById(providers, model.id);
  if (!provider) return model;
  return {
    ...(provider as unknown as TProviderWithModel),
    use_model: model.use_model,
  };
}

export function formatModelLabelForProvider(
  provider: { model_descriptions?: Record<string, string> } | undefined,
  modelName?: string
): string {
  if (!modelName) return '';
  return formatCloudModelLabel(modelName, provider?.model_descriptions);
}
