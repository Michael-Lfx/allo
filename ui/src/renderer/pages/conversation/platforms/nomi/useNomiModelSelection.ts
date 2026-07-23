

import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import { useModelProviderList } from '@/renderer/hooks/agent/useModelProviderList';
import {
  formatModelLabelForProvider,
  hydrateProviderWithModel,
} from '@/renderer/utils/model/cloudModelLabel';
import { useCallback, useEffect, useMemo, useState } from 'react';

export type NomiModelSelection = {
  current_model?: TProviderWithModel;
  providers: IProvider[];
  getAvailableModels: (provider: IProvider) => string[];
  handleSelectModel: (provider: IProvider, modelName: string) => Promise<void>;
  formatModelLabel: (provider: IProvider | undefined, modelName?: string) => string;
  getDisplayModelName: (modelName?: string, provider?: IProvider) => string;
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

  const { providers: allProviders, getAvailableModels, formatModelLabel: catalogFormatLabel } =
    useModelProviderList();

  // Nomicore does not support Google Auth — filter it out
  const providers = useMemo(
    () => allProviders.filter((p) => !p.platform?.toLowerCase().includes('gemini-with-google-auth')),
    [allProviders]
  );

  const hydratedModel = useMemo(
    () => hydrateProviderWithModel(providers, current_model),
    [providers, current_model]
  );

  const formatModelLabel = useCallback(
    (provider: IProvider | undefined, modelName?: string) => {
      if (!modelName) return '';
      const resolved = provider ?? findHydratedProvider(providers, hydratedModel);
      return catalogFormatLabel(resolved, modelName);
    },
    [catalogFormatLabel, hydratedModel, providers]
  );

  const handleSelectModel = useCallback(
    async (provider: IProvider, modelName: string) => {
      const selected = hydrateProviderWithModel(providers, {
        ...(provider as unknown as TProviderWithModel),
        use_model: modelName,
      }) as TProviderWithModel;
      const ok = await onSelectModel(provider, modelName);
      if (ok) {
        setCurrentModel(selected);
      }
    },
    [onSelectModel, providers]
  );

  const getDisplayModelName = useCallback(
    (modelName?: string, provider?: IProvider) => {
      const resolvedName = modelName ?? hydratedModel?.use_model;
      if (!resolvedName) return '';
      const resolvedProvider = provider ?? findHydratedProvider(providers, hydratedModel);
      const label = formatModelLabelForProvider(resolvedProvider, resolvedName);
      const maxLength = 20;
      return label.length > maxLength ? `${label.slice(0, maxLength)}...` : label;
    },
    [hydratedModel, providers]
  );

  return {
    current_model: hydratedModel,
    providers,
    getAvailableModels,
    handleSelectModel,
    formatModelLabel,
    getDisplayModelName,
  };
};

function findHydratedProvider(
  providers: IProvider[],
  model?: TProviderWithModel
): IProvider | undefined {
  return providers.find((p) => p.id === model?.id);
}
