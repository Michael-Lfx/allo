import useSWR from 'swr';

import { ipcBridge } from '@/common';
import type { ModelsDevStatusResponse } from '@/common/types/provider/providerApi';

export const MODELS_DEV_STATUS_SWR_KEY = 'models-dev-status';

export function useModelsDevStatus() {
  const { data, error, mutate, isLoading } = useSWR<ModelsDevStatusResponse | undefined, Error>(
    MODELS_DEV_STATUS_SWR_KEY,
    async () => (await ipcBridge.modelsDev.status.invoke()) ?? undefined,
    {
      revalidateOnFocus: false,
      revalidateOnReconnect: false,
    }
  );

  const refresh = async (force = true) => {
    await ipcBridge.modelsDev.refresh.invoke({ force });
    await mutate();
  };

  return { status: data, error, isLoading, refresh, mutate };
}
