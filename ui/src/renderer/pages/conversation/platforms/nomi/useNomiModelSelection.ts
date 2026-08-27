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
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

export type NomiModelSelection = {
  current_model?: TProviderWithModel;
  isCurrentModelAvailable: boolean;
  isModelCatalogLoading: boolean;
  modelCatalogError?: Error;
  refreshModelCatalog: () => void;
  providers: IProvider[];
  getAvailableModels: (provider: IProvider) => string[];
  handleSelectModel: (provider: IProvider, modelName: string) => Promise<boolean>;
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
  const selectionRequestIdRef = useRef(0);
  const pendingModelKeyRef = useRef<string | null>(null);
  const { t } = useTranslation();

  useEffect(() => {
    // A conversation update can replace the initial model while an older
    // selection is still resolving. That older request must not write its
    // stale family/strategy back into the toolbar.
    selectionRequestIdRef.current += 1;
    pendingModelKeyRef.current = null;
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
    async (provider: IProvider, modelName: string): Promise<boolean> => {
      const modelKey = `${provider.id}:${modelName}`;
      if (pendingModelKeyRef.current === modelKey) return false;
      if (current_model?.id === provider.id && current_model.use_model === modelName) return true;

      const requestId = ++selectionRequestIdRef.current;
      pendingModelKeyRef.current = modelKey;
      const selected = {
        ...(provider as unknown as TProviderWithModel),
        use_model: modelName,
      } as TProviderWithModel;
      try {
        const ok = await onSelectModel(provider, modelName);
        // A later click owns the UI now. Do not show an error for an older
        // request that was intentionally superseded by the user.
        if (requestId !== selectionRequestIdRef.current) return false;
        if (!ok) {
          Message.error(t('agent.model.switchFailed'));
          return false;
        }
        setCurrentModel(selected);
        return true;
      } catch (error) {
        if (requestId !== selectionRequestIdRef.current) return false;
        console.error('[useNomiModelSelection] Failed to switch model:', error);
        Message.error(t('agent.model.switchFailed'));
        return false;
      } finally {
        if (pendingModelKeyRef.current === modelKey && requestId === selectionRequestIdRef.current) {
          pendingModelKeyRef.current = null;
        }
      }
    },
    [current_model?.id, current_model?.use_model, onSelectModel, t]
  );

  const liveCurrentProvider = useMemo(() => {
    if (!current_model?.id) return current_model;
    return providers.find((provider) => provider.id === current_model.id) ?? current_model;
  }, [current_model, providers]);

  const getDisplayModelName = useCallback(
    (modelName?: string) => {
      if (!modelName) return '';
      return formatModelLabel(liveCurrentProvider, modelName);
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
