import { useCallback, useEffect, useState } from 'react';
import { ipcBridge } from '@/common';
import type { IMediaModelList, IMediaModelOption } from '@/common/adapter/ipcBridge';

export const fetchMediaModels = async (): Promise<IMediaModelList> => {
  try {
    return await ipcBridge.media.listModels.invoke();
  } catch {
    return {
      image_models: [],
      video_models: [],
    };
  }
};

export async function refreshMediaModelsCatalog(): Promise<IMediaModelList> {
  return fetchMediaModels();
}

export type UseMediaModelsResult = {
  imageModels: IMediaModelOption[];
  videoModels: IMediaModelOption[];
  isLoading: boolean;
  error: string | null;
  revalidate: () => Promise<IMediaModelList | undefined>;
};

export function useMediaModels(): UseMediaModelsResult {
  const [data, setData] = useState<IMediaModelList | undefined>(undefined);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const revalidate = useCallback(async (): Promise<IMediaModelList | undefined> => {
    setIsLoading(true);
    setError(null);
    try {
      const list = await fetchMediaModels();
      setData(list);
      return list;
    } catch (e) {
      setError(String(e));
      return undefined;
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void revalidate();
  }, [revalidate]);

  return {
    imageModels: data?.image_models ?? [],
    videoModels: data?.video_models ?? [],
    isLoading,
    error,
    revalidate,
  };
}
