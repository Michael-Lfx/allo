/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useMemo } from 'react';
import { modelNamesOf } from '@/common/utils/providerModels';
import { useProvidersQuery } from '@renderer/hooks/agent/useModelProviderList';
import { useModelsForTask, type TaskModelGroup } from '@renderer/hooks/agent/useModelsForTask';
import { useModelSelectorProviderLabel } from '@renderer/hooks/agent/useModelSelectorProviderLabel';
import type { ProviderId } from '@/common/types/ids';

export interface ModelOption {
  providerId: ProviderId;
  providerName: string;
  platform: string;
  model: string;
}

export interface ModelGroup {
  providerId: ProviderId;
  providerName: string;
  platform: string;
  models: ModelOption[];
}

export interface GeneratorModels {
  groups: ModelGroup[];
  flat: ModelOption[];
  /** Any enabled provider exposes at least one usable model at all. */
  hasProviders: boolean;
}

function group(flat: ModelOption[]): ModelGroup[] {
  const groups = new Map<string, ModelGroup>();
  for (const m of flat) {
    let g = groups.get(m.providerId);
    if (!g) {
      g = { providerId: m.providerId, providerName: m.providerName, platform: m.platform, models: [] };
      groups.set(m.providerId, g);
    }
    g.models.push(m);
  }
  return [...groups.values()];
}

function flattenTaskGroups(
  taskGroups: readonly TaskModelGroup[],
  providerLabel: (provider: { name?: string; platform?: string }) => string
): ModelOption[] {
  const flat: ModelOption[] = [];
  for (const { provider, models } of taskGroups) {
    for (const model of models) {
      flat.push({ providerId: provider.id, providerName: providerLabel(provider), platform: provider.platform, model });
    }
  }
  return flat;
}

/** Chat-model catalog for video-generation LLM pickers. */
export function useGeneratorModels(
  mode: 'text',
  options?: { enabled?: boolean }
): GeneratorModels {
  const enabled = options?.enabled ?? true;
  const { data: rawProviders } = useProvidersQuery({ enabled });
  const { groups: chatGroups } = useModelsForTask('chat', undefined, {
    enabled: enabled && mode === 'text',
  });
  const providerLabel = useModelSelectorProviderLabel();

  return useMemo<GeneratorModels>(() => {
    if (!enabled) {
      return { groups: [], flat: [], hasProviders: false };
    }
    const hasProviders =
      (rawProviders ?? []).some((p) => p.enabled !== false && modelNamesOf(p).length > 0) ||
      chatGroups.length > 0;
    const flat = flattenTaskGroups(chatGroups, providerLabel);
    return { groups: group(flat), flat, hasProviders };
  }, [enabled, rawProviders, chatGroups, providerLabel]);
}
