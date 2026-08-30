import { useCallback, useEffect, useState } from 'react';
import { ipcBridge } from '@/common';
import type { IMediaModelList, IMediaModelOption } from '@/common/adapter/ipcBridge';

/** Match backend TTL so concurrent UI paths (home prefs / Canvas sync) share one round-trip. */
const MEDIA_MODELS_CACHE_TTL_MS = 120_000;

const EMPTY_MEDIA_MODEL_LIST: IMediaModelList = {
  image_models: [],
  video_models: [],
  audio_models: [],
};

let mediaModelsCache: { at: number; data: IMediaModelList } | null = null;
let mediaModelsInflight: Promise<IMediaModelList> | null = null;

export const fetchMediaModels = async (): Promise<IMediaModelList> => {
  const now = Date.now();
  if (mediaModelsCache && now - mediaModelsCache.at < MEDIA_MODELS_CACHE_TTL_MS) {
    return mediaModelsCache.data;
  }
  if (mediaModelsInflight) {
    return mediaModelsInflight;
  }
  mediaModelsInflight = (async () => {
    try {
      const data = await ipcBridge.media.listModels.invoke();
      mediaModelsCache = { at: Date.now(), data };
      return data;
    } catch {
      return EMPTY_MEDIA_MODEL_LIST;
    } finally {
      mediaModelsInflight = null;
    }
  })();
  return mediaModelsInflight;
};

/** Bypass TTL and refresh the shared catalog (settings / Model Hub). */
export async function refreshMediaModelsCatalog(): Promise<IMediaModelList> {
  mediaModelsCache = null;
  return fetchMediaModels();
}

export type UseMediaModelsOptions = {
  /** When false, skip network until re-enabled (e.g. preference popover closed). */
  enabled?: boolean;
};

export type UseMediaModelsResult = {
  imageModels: IMediaModelOption[];
  videoModels: IMediaModelOption[];
  audioModels: IMediaModelOption[];
  isLoading: boolean;
  error: string | null;
  revalidate: () => Promise<IMediaModelList | undefined>;
};

export function useMediaModels(options?: UseMediaModelsOptions): UseMediaModelsResult {
  const enabled = options?.enabled ?? true;
  const [data, setData] = useState<IMediaModelList | undefined>(() =>
    mediaModelsCache?.data
  );
  const [isLoading, setIsLoading] = useState(enabled && !mediaModelsCache);
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
    if (!enabled) {
      setIsLoading(false);
      return;
    }
    void revalidate();
  }, [revalidate, enabled]);

  return {
    imageModels: data?.image_models ?? [],
    videoModels: data?.video_models ?? [],
    audioModels: data?.audio_models ?? [],
    isLoading,
    error,
    revalidate,
  };
}
