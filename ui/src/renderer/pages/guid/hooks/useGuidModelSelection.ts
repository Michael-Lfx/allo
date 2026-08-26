/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import type { ConfigKeyMap } from '@/common/config/configKeys';
import { configService } from '@/common/config/configService';
import { useModelsForTask } from '@/renderer/hooks/agent/useModelsForTask';
import { buildChatModelPickerViewModel, type ChatModelPickerViewModel } from '@/renderer/utils/model/chatModelPicker';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

/**
 * Build a unique key for a provider/model pair.
 */
const buildModelKey = (providerId?: string, modelName?: string) => {
  if (!providerId || !modelName) return null;
  return `${providerId}:${modelName}`;
};

/** Provider-based agent keys that share the model list UI */
type ProviderAgentKey = 'nomi';

/** Map agent key → storage key for persisting default model */
const MODEL_STORAGE_KEY: Record<ProviderAgentKey, 'nomi.defaultModel'> = {
  nomi: 'nomi.defaultModel',
};

type PersistedDefaultModel = NonNullable<ConfigKeyMap['nomi.defaultModel']>;

function isPersistedDefaultModel(value: unknown): value is PersistedDefaultModel {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const object = value as Record<string, unknown>;
  return (
    !('id' in object) &&
    typeof object.provider_id === 'string' &&
    typeof object.model === 'string'
  );
}

export type GuidModelSelectionResult = {
  modelList: IProvider[];
  formatGeminiModelLabel: (
    provider: { model_descriptions?: Record<string, string> } | undefined,
    modelName?: string
  ) => string;
  current_model: TProviderWithModel | undefined;
  /** True when a persisted default exists but is no longer in the catalog. */
  defaultModelUnavailable: boolean;
  setCurrentModel: (model_info: TProviderWithModel) => Promise<void>;
  modelPicker: ChatModelPickerViewModel;
  isModelCatalogLoading: boolean;
  modelCatalogError?: Error;
  refreshModelCatalog: () => void;
};

/**
 * Hook that manages the model list and selection state for the Guid page.
 * @param agentKey - current provider-based agent (currently only 'nomi')
 */
export const useGuidModelSelection = (agentKey: ProviderAgentKey = 'nomi'): GuidModelSelectionResult => {
  // Chat-capable catalog from the unified backend resolve — replaces the old
  // duplicate guid/utils/modelUtils name-heuristic implementation.
  const {
    groups,
    isLoading: isModelCatalogLoading,
    error: modelCatalogError,
    refresh: refreshModelCatalog,
  } = useModelsForTask('chat');

  const modelPicker = useMemo(() => buildChatModelPickerViewModel(groups), [groups]);

  const modelList = useMemo(() => groups.map((group) => group.provider), [groups]);
  const modelsByProvider = useMemo(
    () => new Map(groups.map((group) => [group.provider.id, group.models])),
    [groups]
  );

  const availableModelsFor = useCallback(
    (provider: IProvider | undefined): string[] =>
      provider ? (modelsByProvider.get(provider.id) ?? []) : [],
    [modelsByProvider]
  );

  /** Check if a model key still exists in the chat catalog. */
  const isModelKeyAvailable = useCallback(
    (key: string | null): boolean => {
      if (!key) return false;
      return groups.some((group) =>
        group.models.some((modelName) => buildModelKey(group.provider.id, modelName) === key)
      );
    },
    [groups]
  );

  const formatGeminiModelLabel = useCallback(
    (provider: { model_descriptions?: Record<string, string> } | undefined, modelName?: string) =>
      formatModelLabelForProvider(provider, modelName),
    []
  );

  const [current_model, _setCurrentModel] = useState<TProviderWithModel>();
  const [defaultModelUnavailable, setDefaultModelUnavailable] = useState(false);
  const selectedModelKeyRef = useRef<string | null>(null);
  const prevStorageKeyRef = useRef<string | null>(null);

  const storageKey = MODEL_STORAGE_KEY[agentKey];

  const setCurrentModel = useCallback(
    async (model_info: TProviderWithModel, persist = true) => {
      selectedModelKeyRef.current = buildModelKey(model_info.id, model_info.use_model);
      if (persist) {
        await configService.set(storageKey, {
          provider_id: model_info.id,
          model: model_info.use_model,
        }).catch((error) => {
          console.error('Failed to save default model:', error);
        });
      }
      setDefaultModelUnavailable(false);
      _setCurrentModel(model_info);
    },
    [storageKey]
  );

  // Set default model when modelList or agent changes
  useEffect(() => {
    const setDefaultModel = async () => {
      if (!modelList || modelList.length === 0) {
        return;
      }
      // When agent switches, reset selection so we reload from the new storage key
      const agentChanged = prevStorageKeyRef.current !== null && prevStorageKeyRef.current !== storageKey;
      prevStorageKeyRef.current = storageKey;
      if (agentChanged) {
        selectedModelKeyRef.current = null;
      }

      const currentKey = selectedModelKeyRef.current || buildModelKey(current_model?.id, current_model?.use_model);
      if (!agentChanged && isModelKeyAvailable(currentKey)) {
        if (!selectedModelKeyRef.current && currentKey) {
          selectedModelKeyRef.current = currentKey;
        }
        return;
      }
      const rawSavedModel: unknown = configService.get(storageKey);
      const savedModel = isPersistedDefaultModel(rawSavedModel) ? rawSavedModel : undefined;
      if (rawSavedModel !== undefined && savedModel === undefined) {
        console.warn(`Ignoring invalid persisted default model for ${storageKey}; no legacy migration is performed.`);
      }

      const exactMatch = savedModel
        ? modelList.find((provider) => provider.id === savedModel.provider_id)
        : undefined;
      const resolvedUseModel =
        exactMatch && savedModel && availableModelsFor(exactMatch).includes(savedModel.model)
          ? savedModel.model
          : undefined;

      if (!exactMatch || !resolvedUseModel) {
        // A missing/invalid preference is a user decision boundary. Do not
        // silently replace it with the first provider: that can send a new
        // conversation to an unintended account and also rewrite the user's
        // persisted choice. Explicit selection remains the only path that
        // updates nomi.defaultModel.
        selectedModelKeyRef.current = null;
        _setCurrentModel(undefined);
        setDefaultModelUnavailable(rawSavedModel !== undefined);
        return;
      }

      setDefaultModelUnavailable(false);
      await setCurrentModel(
        {
          ...exactMatch,
          use_model: resolvedUseModel,
        },
        false,
      );
    };

    setDefaultModel().catch((error) => {
      console.error('Failed to set default model:', error);
    });
    // availableModelsFor / isModelKeyAvailable derive from the same catalog
    // groups as modelList, so modelList is the single change signal.
  }, [modelList, storageKey]);

  return {
    modelList,
    formatGeminiModelLabel,
    current_model,
    defaultModelUnavailable,
    setCurrentModel,
    modelPicker,
    isModelCatalogLoading,
    modelCatalogError,
    refreshModelCatalog,
  };
};
