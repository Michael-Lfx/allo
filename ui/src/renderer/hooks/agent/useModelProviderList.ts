import { ipcBridge } from '@/common';
import { FLOWY_BUILTIN_PROVIDER_ID, GOOGLE_AUTH_PROVIDER_ID, SERVER_MANAGED_MODELS } from '@/common/config/constants';
import type { IProvider } from '@/common/config/storage';
import { useCallback, useEffect, useMemo, useRef } from 'react';
import useSWR, { type SWRConfiguration } from 'swr';
import { useGoogleAuthModels } from './useGoogleAuthModels';
import { hasSpecificModelCapability } from '@/renderer/utils/model/modelCapabilities';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { orderModelSelectorProviders } from './modelSelectorProviderOrdering';

export interface ModelProviderListResult {
  providers: IProvider[];
  configuredProviders: IProvider[];
  isLoading: boolean;
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

export const useProvidersQuery = () => {
  return useSWR<IProvider[]>(PROVIDERS_SWR_KEY, fetchProviders, PROVIDERS_SWR_OPTIONS);
};

/**
 * Shared hook that builds the provider list (including Google Auth)
 * and exposes helpers consumed by both conversation and channel settings.
 */
export const useModelProviderList = (): ModelProviderListResult => {
  const { isGoogleAuth, isLoading: isGoogleAuthLoading } = useGoogleAuthModels();

  const { data: modelConfig, isLoading: isProvidersLoading } = useProvidersQuery();

  // Mutable cache for available-model filtering
  const available_modelsCacheRef = useRef(new Map<string, string[]>());

  // 当 modelConfig 变化时清除缓存
  useEffect(() => {
    available_modelsCacheRef.current.clear();
  }, [modelConfig]);

  const getAvailableModels = useCallback((provider: IProvider): string[] => {
    // 包含 model_enabled 状态到缓存 key 中
    const model_enabledKey = provider.model_enabled ? JSON.stringify(provider.model_enabled) : 'all-enabled';
    const cacheKey = `${provider.id}-${(provider.models || []).join(',')}-${model_enabledKey}`;
    const cache = available_modelsCacheRef.current;
    if (cache.has(cacheKey)) {
      return cache.get(cacheKey)!;
    }
    const result: string[] = [];
    for (const modelName of provider.models || []) {
      // 检查模型是否被禁用（默认为启用）
      const isModelEnabled = provider.model_enabled?.[modelName] !== false;
      if (!isModelEnabled) continue;

      const functionCalling = hasSpecificModelCapability(provider, modelName, 'function_calling');
      const excluded = hasSpecificModelCapability(provider, modelName, 'excludeFromPrimary');
      if ((functionCalling === true || functionCalling === undefined) && excluded !== true) {
        result.push(modelName);
      }
    }
    cache.set(cacheKey, result);
    return result;
  }, []);

  const configuredProviders = useMemo(() => {
    const list: IProvider[] = Array.isArray(modelConfig) ? modelConfig : [];
    // Server-managed mode only exposes Flowy Cloud; skip virtual Google Auth.
    if (SERVER_MANAGED_MODELS || !isGoogleAuth) {
      return list;
    }
    const googleProvider: IProvider = {
      id: GOOGLE_AUTH_PROVIDER_ID,
      name: 'Gemini Google Auth',
      platform: 'gemini-with-google-auth',
      base_url: '',
      api_key: '',
      model: [],
      capabilities: [{ type: 'text' }, { type: 'vision' }, { type: 'function_calling' }],
      enabled: true, // Google Auth provider 始终启用
    } as unknown as IProvider;
    return [googleProvider, ...list];
  }, [isGoogleAuth, modelConfig]);

  const providers = useMemo(() => {
    // 过滤掉被禁用的 provider（默认为启用）
    const list = configuredProviders.filter((p) => p.enabled !== false);
    if (SERVER_MANAGED_MODELS) {
      return list.filter((p) => p.id === FLOWY_BUILTIN_PROVIDER_ID && getAvailableModels(p).length > 0);
    }
    // 过滤掉没有可用模型的 provider
    return orderModelSelectorProviders(list.filter((p) => getAvailableModels(p).length > 0));
  }, [configuredProviders, getAvailableModels]);

  const formatModelLabel = useCallback(
    (provider: { platform?: string; model_descriptions?: Record<string, string> } | undefined, modelName?: string) => {
      if (!modelName) return '';
      return formatCloudModelLabel(modelName, provider?.model_descriptions);
    },
    []
  );

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
