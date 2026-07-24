/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Shared Flowy image/video model catalog for media settings and video generation.
 * Backend `/api/media/models` has no server-side list cache; this SWR layer is the
 * only client cache — revalidated on every mount of a consumer (model selector pages).
 */

import { ipcBridge } from '@/common';
import type { IMediaModelList } from '@/common/adapter/ipcBridge';
import { useCallback, useEffect } from 'react';
import useSWR, { mutate, type SWRConfiguration } from 'swr';

export const MEDIA_MODELS_SWR_KEY = 'media/models';

export const MEDIA_MODELS_SWR_OPTIONS: SWRConfiguration<IMediaModelList, Error> = {
  revalidateOnFocus: false,
  revalidateOnReconnect: false,
  revalidateOnMount: true,
  shouldRetryOnError: false,
};

let mediaAutoRefreshPromise: Promise<void> | null = null;

export const fetchMediaModels = async (): Promise<IMediaModelList> => {
  return (
    (await ipcBridge.media.listModels.invoke()) ?? {
      image_models: [],
      video_models: [],
    }
  );
};

export async function refreshMediaModelsCatalog(): Promise<IMediaModelList> {
  const list = await fetchMediaModels();
  // Replace cache without a follow-up revalidate so a slower in-flight fetch
  // cannot resurrect a previous (longer) model list after delisting.
  await mutate(MEDIA_MODELS_SWR_KEY, list, { revalidate: false });
  return list;
}

export async function refreshMediaModelsCatalogIfStale(): Promise<void> {
  if (mediaAutoRefreshPromise) {
    await mediaAutoRefreshPromise;
    return;
  }

  mediaAutoRefreshPromise = refreshMediaModelsCatalog()
    .then(() => undefined)
    .catch((error) => {
      console.warn('[media] Failed to refresh image/video model catalog:', error);
    })
    .finally(() => {
      mediaAutoRefreshPromise = null;
    });
  await mediaAutoRefreshPromise;
}

export type UseMediaModelsResult = {
  imageModels: string[];
  videoModels: string[];
  isLoading: boolean;
  error: unknown;
  revalidate: () => Promise<IMediaModelList | undefined>;
};

/**
 * Canonical hook for Flowy image/video model lists. Prefer this over calling
 * `ipcBridge.media.listModels` directly so mount-time revalidation is shared.
 */
export function useMediaModels(): UseMediaModelsResult {
  const { data, isLoading, error } = useSWR<IMediaModelList>(
    MEDIA_MODELS_SWR_KEY,
    fetchMediaModels,
    MEDIA_MODELS_SWR_OPTIONS
  );

  useEffect(() => {
    void refreshMediaModelsCatalogIfStale();
  }, []);

  const revalidate = useCallback(() => mutate<IMediaModelList>(MEDIA_MODELS_SWR_KEY), []);

  return {
    imageModels: data?.image_models ?? [],
    videoModels: data?.video_models ?? [],
    isLoading: isLoading || data === undefined,
    error,
    revalidate,
  };
}
