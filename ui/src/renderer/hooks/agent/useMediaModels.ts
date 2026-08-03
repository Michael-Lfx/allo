/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Shared Flowy image/video model catalog for media settings and video generation.
 * Always hits `/api/media/models` (which proxies the cloud catalog) — no client-side
 * list cache. Each mount / explicit refresh fetches the latest ids.
 */

import { ipcBridge } from '@/common';
import type { IMediaModelList } from '@/common/adapter/ipcBridge';
import { useCallback, useEffect, useState } from 'react';

export const fetchMediaModels = async (): Promise<IMediaModelList> => {
  return (
    (await ipcBridge.media.listModels.invoke()) ?? {
      image_models: [],
      video_models: [],
    }
  );
};

/** Always request a fresh catalog from the backend (no shared cache). */
export async function refreshMediaModelsCatalog(): Promise<IMediaModelList> {
  return fetchMediaModels();
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
 * `ipcBridge.media.listModels` directly so every consumer refreshes on mount.
 */
export function useMediaModels(): UseMediaModelsResult {
  const [data, setData] = useState<IMediaModelList | undefined>(undefined);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);

  const revalidate = useCallback(async (): Promise<IMediaModelList | undefined> => {
    setIsLoading(true);
    try {
      const list = await fetchMediaModels();
      setData(list);
      setError(null);
      return list;
    } catch (err) {
      setError(err);
      console.warn('[media] Failed to fetch image/video model catalog:', err);
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
    isLoading: isLoading || data === undefined,
    error,
    revalidate,
  };
}
