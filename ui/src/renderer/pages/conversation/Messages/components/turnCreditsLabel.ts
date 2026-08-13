/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TurnCreditUsageData } from '@/common/config/storage';
import { modelNamesOf } from '@/common/utils/providerModels';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';

type CreditLabelProvider = Pick<IProvider, 'models' | 'models_detail' | 'model_descriptions'>;

const collectCreditsByModel = (calls: TurnCreditUsageData['calls']): Map<string, number> => {
  const creditsByModel = new Map<string, number>();
  for (const call of calls ?? []) {
    const modelName = call.modelName.trim() || 'model';
    creditsByModel.set(modelName, (creditsByModel.get(modelName) ?? 0) + call.creditConsumed);
  }
  return creditsByModel;
};

/** Map a billed model id onto the catalog id the model list would format. */
export const resolveCatalogModelName = (
  billedName: string,
  provider?: CreditLabelProvider
): string => {
  const trimmed = billedName.trim();
  if (!trimmed || !provider) return trimmed;

  const candidates = [trimmed];
  if (/^AIPC-/i.test(trimmed)) {
    candidates.push(trimmed.replace(/^AIPC-/i, ''));
  } else {
    candidates.push(`AIPC-${trimmed}`);
  }

  const names = modelNamesOf(provider);
  const descriptions = provider.model_descriptions ?? {};
  for (const candidate of candidates) {
    if (names.includes(candidate) || descriptions[candidate]) return candidate;
  }

  const lowered = new Set(candidates.map((candidate) => candidate.toLowerCase()));
  return names.find((name) => lowered.has(name.toLowerCase())) ?? trimmed;
};

export const formatTurnCreditModelLabel = (
  modelName: string,
  provider?: CreditLabelProvider
): string => formatModelLabelForProvider(provider, resolveCatalogModelName(modelName, provider));

export const formatTurnCreditDetails = (
  calls: TurnCreditUsageData['calls'],
  provider?: CreditLabelProvider
): string | undefined => {
  const creditsByModel = collectCreditsByModel(calls);
  if (creditsByModel.size === 0) return undefined;
  return [...creditsByModel]
    .map(([modelName, credits]) => `${formatTurnCreditModelLabel(modelName, provider)}: ${credits}`)
    .join('\n');
};

export const formatTurnCreditModels = (
  calls: TurnCreditUsageData['calls'],
  fallbackModel?: string,
  provider?: CreditLabelProvider
): string | undefined => {
  const creditsByModel = collectCreditsByModel(calls);
  const names = [...creditsByModel.keys()];
  const fallback = fallbackModel?.trim();
  if (names.length === 0 && fallback) names.push(fallback);
  if (names.length === 0) return undefined;
  return names.map((name) => formatTurnCreditModelLabel(name, provider)).join(', ');
};
