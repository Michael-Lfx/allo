import { ipcBridge } from '@/common';
import useSWR from 'swr';

export const MANAGED_FREE_MODELS_CAPABILITY_SWR_KEY = 'system-info:managed-free-models';

const fetchManagedFreeModelsEnabled = async (): Promise<boolean> => {
  const info = await ipcBridge.application.systemInfo.invoke();
  return info.managedFreeModelsEnabled === true;
};

/**
 * Process capability, not a user preference. `enabled` is deliberately false
 * while the capability request is unresolved so the preserved free-model UI
 * cannot issue IPC calls during the startup race.
 */
export const useManagedFreeModelsEnabled = () => {
  const query = useSWR<boolean>(MANAGED_FREE_MODELS_CAPABILITY_SWR_KEY, fetchManagedFreeModelsEnabled, {
    revalidateOnFocus: false,
    revalidateOnReconnect: false,
    shouldRetryOnError: false,
  });

  return {
    enabled: query.data === true,
    isLoading: query.data === undefined && !query.error,
  };
};
