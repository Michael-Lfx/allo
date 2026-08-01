import { ipcBridge } from '@/common';
import { GOOGLE_AUTH_PROVIDER_ID } from '@/common/config/constants';
import type { IProvider } from '@/common/config/storage';
import { useCallback, useMemo } from 'react';
import useSWR, { mutate, type SWRConfiguration } from 'swr';
import { useGoogleAuthModels } from './useGoogleAuthModels';
import { orderModelSelectorProviders } from './modelSelectorProviderOrdering';

export interface ModelProviderListResult {
  /** Enabled providers in selector order — provider METADATA only. Which
   * models a surface may list comes from `useModelsForTask` (catalog resolve);
   * the old name-heuristic `getAvailableModels` filter is gone. */
  providers: IProvider[];
  configuredProviders: IProvider[];
  isLoading: boolean;
  /** Enabled models on a provider row (no capability heuristics). Prefer
   * `useModelsForTask` for task-filtered pickers. */
  getAvailableModels: (provider: IProvider) => string[];
  formatModelLabel: (provider: { platform?: string } | undefined, modelName?: string) => string;
}

export const PROVIDERS_SWR_KEY = 'providers';

// Provider config is local application state. Keep it stable after the initial
// load and refresh only through explicit mutate() calls after CRUD operations.
export const PROVIDERS_SWR_OPTIONS: SWRConfiguration<IProvider[], Error> = {
  revalidateOnFocus: false,
  revalidateOnReconnect: false,
  shouldRetryOnError: false,
};

export const fetchProviders = async (): Promise<IProvider[]> => {
  return (await ipcBridge.mode.listProviders.invoke()) ?? [];
};

/**
 * Optional cloud catalog sync then replace the shared providers SWR cache.
 * Soft-fails when cloud sync is unavailable (not logged in / no cloud host).
 */
export async function refreshProvidersCatalog(): Promise<IProvider[]> {
  try {
    await ipcBridge.cloud.syncModels.invoke();
  } catch (error) {
    console.warn('[providers] Failed to sync chat model catalog:', error);
  }
  const providers = await fetchProviders();
  await mutate(PROVIDERS_SWR_KEY, providers, { revalidate: false });
  return providers;
}

export const useProvidersQuery = () => {
  return useSWR<IProvider[]>(PROVIDERS_SWR_KEY, fetchProviders, PROVIDERS_SWR_OPTIONS);
};

/**
 * Shared hook that builds the provider list (including Google Auth) and
 * exposes provider metadata/label helpers. Task-capable MODEL lists are
 * resolved by `useModelsForTask` against the backend catalog — this hook
 * deliberately no longer filters models by capability name heuristics.
 */
export const useModelProviderList = (): ModelProviderListResult => {
  const { isGoogleAuth, isLoading: isGoogleAuthLoading } = useGoogleAuthModels();

  const { data: modelConfig, isLoading: isProvidersLoading } = useProvidersQuery();

  const configuredProviders = useMemo(() => {
    const list: IProvider[] = Array.isArray(modelConfig) ? modelConfig : [];
    if (isGoogleAuth) {
      const googleProvider: IProvider = {
        id: GOOGLE_AUTH_PROVIDER_ID,
        name: 'Gemini Google Auth',
        platform: 'gemini-with-google-auth',
        base_url: '',
        api_key: '',
        model: [],
        enabled: true, // Google Auth provider 始终启用
      } as unknown as IProvider;
      return [googleProvider, ...list];
    }
    return list;
  }, [isGoogleAuth, modelConfig]);

  const providers = useMemo(() => {
    // 过滤掉被禁用的 provider（默认为启用）。
    // 注意：不再按「是否有可用模型」过滤 —— 模型级别的可用性由
    // useModelsForTask（后端 catalog resolve）决定，空组不会被渲染。
    return orderModelSelectorProviders(configuredProviders.filter((p) => p.enabled !== false));
  }, [configuredProviders]);

  const getAvailableModels = useCallback((provider: IProvider): string[] => {
    return (provider.models || []).filter((modelName) => provider.model_enabled?.[modelName] !== false);
  }, []);

  const formatModelLabel = useCallback((_provider: { platform?: string } | undefined, modelName?: string) => {
    if (!modelName) return '';
    return modelName;
  }, []);

  return {
    providers,
    configuredProviders,
    // SWR clears `isLoading` after an error while `data` stays undefined. Keep
    // the catalog unresolved in that state so consumers never reinterpret a
    // failed provider request as an authoritative empty catalog and purge every
    // persisted model reference.
    isLoading: isProvidersLoading || isGoogleAuthLoading || !Array.isArray(modelConfig),
    getAvailableModels,
    formatModelLabel,
  };
};
