/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, ModelTrait } from '@/common/config/storage';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/types/ids';
import type { ProviderModelResponse, ModelHealthStatus } from '@/common/types/provider/providerModel';
import { compositeKey } from '@/common/utils/compositeKey';
import { modelHealthOf } from '@/common/utils/providerModels';
import type { TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';
import { catalogCreditRateForModel } from './creditRate';
import { formatModelLabelForProvider } from './cloudModelLabel';
import { catalogReasoningEffortForModel } from './reasoningEffort';

export const FLOWY_CATALOG_FAMILY_PARAM = '_flowy_catalog_family';
export const FLOWY_CATALOG_AUTO_TIER_PARAM = '_flowy_catalog_auto_tier';

export type ChatModelFamily = 'auto' | 'cloud' | 'provider';
export type AutoTier = 'intelligence' | 'balance' | 'cost';

export const AUTO_TIER_ORDER: readonly AutoTier[] = ['intelligence', 'balance', 'cost'];

export const AUTO_TIER_LABEL_FALLBACK: Record<AutoTier, string> = {
  intelligence: 'Smart',
  balance: 'Balanced',
  cost: 'Economy',
};

export interface ChatModelOption {
  key: string;
  provider: IProvider;
  model: string;
  label: string;
  family: ChatModelFamily;
  autoTier?: AutoTier;
  reasoningLevels: string[];
  creditRate?: number;
  supportsVision: boolean;
  supportsTools: boolean;
  health?: ModelHealthStatus;
  disabled?: boolean;
  disabledReason?: 'vision_required';
}

export interface ChatModelPickerViewModel {
  autoModels: ChatModelOption[];
  cloudModels: ChatModelOption[];
  otherProviderGroups: TaskModelGroup[];
}

export type ChatModelPickerOptions = {
  hasImageAttachments?: boolean;
};

const isModelTrait = (value: ModelTrait, expected: ModelTrait): boolean => value === expected;

const detailForModel = (provider: IProvider, model: string): ProviderModelResponse | undefined =>
  provider.models_detail?.find((detail) => detail.model === model);

const stringParam = (params: unknown, key: string): string | undefined => {
  if (!params || typeof params !== 'object' || Array.isArray(params)) return undefined;
  const value = (params as Record<string, unknown>)[key];
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
};

const autoTierOf = (value: string | undefined): AutoTier | undefined => {
  if (value === 'intelligence' || value === 'balance' || value === 'cost') return value;
  return undefined;
};

const familyOf = (provider: IProvider, detail: ProviderModelResponse | undefined): ChatModelFamily => {
  if (provider.id !== FLOWY_BUILTIN_PROVIDER_ID) return 'provider';
  return stringParam(detail?.params, FLOWY_CATALOG_FAMILY_PARAM) === 'auto' ? 'auto' : 'cloud';
};

const modelOption = (
  provider: IProvider,
  model: string,
  options: ChatModelPickerOptions
): ChatModelOption => {
  const detail = detailForModel(provider, model);
  const family = familyOf(provider, detail);
  const autoTier = family === 'auto' ? autoTierOf(stringParam(detail?.params, FLOWY_CATALOG_AUTO_TIER_PARAM)) : undefined;
  const traits = detail?.traits ?? [];
  const supportsVision = traits.some((trait) => isModelTrait(trait, 'vision_input'));
  const supportsTools = traits.some((trait) => isModelTrait(trait, 'function_calling'));
  const hasImageAttachments = options.hasImageAttachments === true;

  return {
    key: compositeKey(provider.id, model),
    provider,
    model,
    label: formatModelLabelForProvider(provider, model),
    family,
    autoTier,
    // Auto switches real model ids through its tier selector; it never
    // exposes Cloud's reasoning-effort control, even while stale catalog
    // metadata is being cleared by the next provider sync.
    reasoningLevels: family === 'auto' ? [] : catalogReasoningEffortForModel(provider, model),
    creditRate: catalogCreditRateForModel(provider, model),
    supportsVision,
    supportsTools,
    health: modelHealthOf(provider, model),
    disabled: hasImageAttachments && !supportsVision,
    disabledReason: hasImageAttachments && !supportsVision ? 'vision_required' : undefined,
  };
};

export const buildChatModelPickerViewModel = (
  groups: readonly TaskModelGroup[],
  options: ChatModelPickerOptions = {}
): ChatModelPickerViewModel => {
  const autoModels: ChatModelOption[] = [];
  const cloudModels: ChatModelOption[] = [];
  const otherProviderGroups: TaskModelGroup[] = [];

  for (const group of groups) {
    if (group.provider.id !== FLOWY_BUILTIN_PROVIDER_ID) {
      otherProviderGroups.push(group);
      continue;
    }

    for (const model of group.models) {
      const option = modelOption(group.provider, model, options);
      if (option.family === 'auto') {
        autoModels.push(option);
      } else {
        cloudModels.push(option);
      }
    }
  }

  autoModels.sort((left, right) => {
    const leftIndex = left.autoTier ? AUTO_TIER_ORDER.indexOf(left.autoTier) : AUTO_TIER_ORDER.length;
    const rightIndex = right.autoTier ? AUTO_TIER_ORDER.indexOf(right.autoTier) : AUTO_TIER_ORDER.length;
    return leftIndex - rightIndex || left.label.localeCompare(right.label);
  });

  return { autoModels, cloudModels, otherProviderGroups };
};

export const allChatModelOptions = (
  viewModel: ChatModelPickerViewModel,
  options: ChatModelPickerOptions = {}
): ChatModelOption[] => [
  ...viewModel.autoModels.map((option) =>
    options.hasImageAttachments === undefined ? option : modelOption(option.provider, option.model, options)
  ),
  ...viewModel.cloudModels.map((option) =>
    options.hasImageAttachments === undefined ? option : modelOption(option.provider, option.model, options)
  ),
  ...viewModel.otherProviderGroups.flatMap((group) =>
    group.models.map((model) => modelOption(group.provider, model, options))
  ),
];

export const findChatModelOption = (
  viewModel: ChatModelPickerViewModel,
  providerId?: string,
  model?: string,
  options: ChatModelPickerOptions = {}
): ChatModelOption | undefined => {
  if (!providerId || !model) return undefined;
  return allChatModelOptions(viewModel, options).find(
    (option) => option.provider.id === providerId && option.model === model
  );
};
