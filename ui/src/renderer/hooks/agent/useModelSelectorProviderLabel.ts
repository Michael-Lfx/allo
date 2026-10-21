import { HERDSMAN_PLATFORM } from '@/common/types/provider/herdsman';
import { NOMIFUN_FREE_MODEL_PLATFORM } from '@/common/types/provider/managedModelService';
import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';

type ModelSelectorProvider = {
  name?: string;
  platform?: string;
};

type ManagedModelProviderLabels = {
  free: string;
  herdsman: string;
};

export const formatModelSelectorProviderLabel = (
  provider: ModelSelectorProvider,
  labels: ManagedModelProviderLabels,
): string => {
  if (provider.platform === HERDSMAN_PLATFORM) return labels.herdsman;
  if (provider.platform === NOMIFUN_FREE_MODEL_PLATFORM) return labels.free;
  return provider.name?.trim() || provider.platform?.trim() || '';
};

/** Localized provider label for every model-selection surface. */
export const useModelSelectorProviderLabel = () => {
  const { t } = useTranslation();
  const free = t('settings.modelHub.free.title');
  const herdsman = t('settings.modelHub.herdsman.title');

  return useCallback(
    (provider: ModelSelectorProvider) => formatModelSelectorProviderLabel(provider, { free, herdsman }),
    [free, herdsman],
  );
};
