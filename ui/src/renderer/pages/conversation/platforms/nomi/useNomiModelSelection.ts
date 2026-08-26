/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import { isManagedModelProvider } from '@/common/types/provider/managedModelService';
import { useModelProviderList } from '@/renderer/hooks/agent/useModelProviderList';
import { useModelsForTask } from '@/renderer/hooks/agent/useModelsForTask';
import { buildChatModelPickerViewModel, type ChatModelPickerViewModel } from '@/renderer/utils/model/chatModelPicker';
import { useCallback, useEffect, useMemo, useState } from 'react';

export type NomiModelSelection = {
  current_model?: TProviderWithModel;
  isCurrentModelAvailable: boolean;
  isModelCatalogLoading: boolean;
  modelCatalogError?: Error;
  refreshModelCatalog: () => void;
  providers: IProvider[];
  getAvailableModels: (provider: IProvider) => string[];
  handleSelectModel: (provider: IProvider, modelName: string) => Promise<void>;
  formatModelLabel: (
    provider: { model_descriptions?: Record<string, string> } | undefined,
    modelName?: string
  ) => string;
  getDisplayModelName: (modelName?: string) => string;
  modelPicker: ChatModelPickerViewModel;
};

export type UseNomiModelSelectionOptions = {
  initialModel: TProviderWithModel | undefined;
  onSelectModel: (provider: IProvider, modelName: string) => Promise<boolean>;
};

export const useNomiModelSelection = ({
  initialModel,
  onSelectModel,
}: UseNomiModelSelectionOptions): NomiModelSelection => {
  const [current_model, setCurrentModel] = useState<TProviderWithModel | undefined>(initialModel);

  useEffect(() => {
    setCurrentModel(initialModel);
  }, [initialModel?.id, initialModel?.use_model]);

  const { formatModelLabel } = useModelProviderList();
  // Unified chat catalog — the model list comes from the backend resolve
  // (profiles + inference), not from frontend name heuristics.
  const { groups, isLoading: isModelCatalogLoading, error: modelCatalogError, refresh: refreshModelCatalog } =
    useModelsForTask('chat');

  // Nomicore does not support Google Auth — filter it out.
  // Managed free models stay off the conversation picker; keep them in the
  // catalog map so a currently selected free model still counts as available.
  const providers = useMemo(
    () =>
      groups
        .map((group) => group.provider)
        .filter((p) => !p.platform?.toLowerCase().includes('gemini-with-google-auth'))
        .filter((p) => !isManagedModelProvider(p)),
    [groups]
  );

  const visibleGroups = useMemo(
    () =>
      groups.filter(
        (group) =>
          !group.provider.platform?.toLowerCase().includes('gemini-with-google-auth') &&
          !isManagedModelProvider(group.provider)
      ),
    [groups]
  );

  const modelPicker = useMemo(() => buildChatModelPickerViewModel(visibleGroups), [visibleGroups]);

  const modelsByProvider = useMemo(
    () => new Map(groups.map((group) => [group.provider.id, group.models])),
    [groups]
  );

  const getAvailableModels = useCallback(
    (provider: IProvider): string[] => modelsByProvider.get(provider.id) ?? [],
    [modelsByProvider]
  );

  const isCurrentModelAvailable = useMemo(() => {
    if (!current_model?.id || !current_model.use_model) return false;
    return modelsByProvider.get(current_model.id)?.includes(current_model.use_model) ?? false;
  }, [current_model?.id, current_model?.use_model, modelsByProvider]);

  const handleSelectModel = useCallback(
    async (provider: IProvider, modelName: string) => {
      const selected = {
        ...(provider as unknown as TProviderWithModel),
        use_model: modelName,
      } as TProviderWithModel;
      const ok = await onSelectModel(provider, modelName);
      if (ok) {
        setCurrentModel(selected);
      }
    },
    [onSelectModel]
  );

  const liveCurrentProvider = useMemo(() => {
    if (!current_model?.id) return current_model;
    return providers.find((provider) => provider.id === current_model.id) ?? current_model;
  }, [current_model, providers]);

  const getDisplayModelName = useCallback(
    (modelName?: string) => {
      if (!modelName) return '';
      const label = formatModelLabel(liveCurrentProvider, modelName);
      const maxLength = 20;
      return label.length > maxLength ? `${label.slice(0, maxLength)}...` : label;
    },
    [formatModelLabel, liveCurrentProvider]
  );

  return {
    current_model,
    isCurrentModelAvailable,
    isModelCatalogLoading,
    modelCatalogError,
    refreshModelCatalog,
    providers,
    getAvailableModels,
    handleSelectModel,
    formatModelLabel,
    getDisplayModelName,
    modelPicker,
  };
};
