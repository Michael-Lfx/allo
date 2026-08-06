import { ipcBridge } from '@/common';
import type { IProvider } from '@/common/config/storage';
import { formatModelLabelForProvider } from '@/renderer/utils/model/cloudModelLabel';
import { useCallback, useEffect, useMemo } from 'react';
import useSWR, { mutate, type SWRConfiguration } from 'swr';
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
  formatModelLabel: (
    provider: { model_descriptions?: Record<string, string> } | undefined,
    modelName?: string
  ) => string;
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

export interface ProviderCatalogRefreshResult {
  providers: IProvider[];
  syncError?: Error;
  stale?: boolean;
}

const isModelCatalogCacheKey = (key: unknown): boolean =>
  key === PROVIDERS_SWR_KEY || (typeof key === 'string' && key.startsWith('models-for-task:'));

let providerCatalogGeneration = 0;

/** Clear all provider/model catalog entries before an account or generation transition. */
export async function clearAvailableModelsCache(): Promise<void> {
  providerCatalogGeneration += 1;
  await mutate(isModelCatalogCacheKey, undefined, { revalidate: false });
}

/**
 * Optional cloud catalog sync then replace the shared providers SWR cache.
 * The provider list is still returned when cloud sync fails so callers can
 * distinguish a usable cached catalog (`syncError` + models) from a truly
 * empty model environment.
 */
export async function refreshProvidersCatalog(): Promise<ProviderCatalogRefreshResult> {
  await clearAvailableModelsCache();
  const requestGeneration = providerCatalogGeneration;
  let syncError: Error | undefined;
  try {
    const syncResult = await ipcBridge.cloud.syncModels.invoke();
    if (!syncResult?.synced) {
      throw new Error('The cloud model catalog was not synchronized');
    }
  } catch (error) {
    syncError = error instanceof Error ? error : new Error(String(error));
    console.warn('[providers] Failed to sync chat model catalog:', syncError);
  }
  const providers = await fetchProviders();
  if (requestGeneration !== providerCatalogGeneration) {
    return { providers, syncError, stale: true };
  }
  await mutate(PROVIDERS_SWR_KEY, providers, { revalidate: false });
  await mutate(
    (key: unknown) => typeof key === 'string' && key.startsWith('models-for-task:'),
    undefined,
    { revalidate: true }
  );
  return { providers, syncError };
}

// Deduplicate concurrent selector mounts without introducing a time-based
// cache: every fresh mount still gets a fresh catalog sync.
let providersAutoRefreshPromise: Promise<ProviderCatalogRefreshResult> | null = null;

export function refreshProvidersCatalogIfStale(): Promise<ProviderCatalogRefreshResult> {
  if (!providersAutoRefreshPromise) {
    providersAutoRefreshPromise = refreshProvidersCatalog().finally(() => {
      providersAutoRefreshPromise = null;
    });
  }
  return providersAutoRefreshPromise;
}

export const useProvidersQuery = () => {
  return useSWR<IProvider[]>(PROVIDERS_SWR_KEY, fetchProviders, PROVIDERS_SWR_OPTIONS);
};

/**
 * Shared hook that builds the provider list and exposes provider
 * metadata/label helpers. Task-capable MODEL lists are
 * resolved by `useModelsForTask` against the backend catalog — this hook
 * deliberately no longer filters models by capability name heuristics.
 */
export const useModelProviderList = (): ModelProviderListResult => {
  const { data: modelConfig, isLoading: isProvidersLoading } = useProvidersQuery();

  useEffect(() => {
    void refreshProvidersCatalogIfStale().catch((error) => {
      console.warn('[providers] Automatic catalog refresh failed:', error);
    });
  }, []);

  const configuredProviders = useMemo(() => {
    const list: IProvider[] = Array.isArray(modelConfig) ? modelConfig : [];
    return list;
  }, [modelConfig]);

  const providers = useMemo(() => {
    // 过滤掉被禁用的 provider（默认为启用）。
    // 注意：不再按「是否有可用模型」过滤 —— 模型级别的可用性由
    // useModelsForTask（后端 catalog resolve）决定，空组不会被渲染。
    return orderModelSelectorProviders(configuredProviders.filter((p) => p.enabled !== false));
  }, [configuredProviders]);

  const getAvailableModels = useCallback((provider: IProvider): string[] => {
    return (provider.models || []).filter((modelName) => provider.model_enabled?.[modelName] !== false);
  }, []);

  const formatModelLabel = useCallback(
    (provider: { model_descriptions?: Record<string, string> } | undefined, modelName?: string) =>
      formatModelLabelForProvider(provider, modelName),
    []
  );

  return {
    providers,
    configuredProviders,
    // SWR clears `isLoading` after an error while `data` stays undefined. Keep
    // the catalog unresolved in that state so consumers never reinterpret a
    // failed provider request as an authoritative empty catalog and purge every
    // persisted model reference.
    isLoading: isProvidersLoading || !Array.isArray(modelConfig),
    getAvailableModels,
    formatModelLabel,
  };
};
