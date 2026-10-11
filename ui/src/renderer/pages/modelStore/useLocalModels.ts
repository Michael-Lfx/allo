/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Message } from '@arco-design/web-react';
import useSWR, { mutate } from 'swr';
import { ipcBridge } from '@/common';
import type { LocalModelStatus } from '@common/types/localModel';
import catalog from './catalog.json';
import { diffDownloadErrors, formatDownloadError } from './format';
import type { ModelStoreCatalogEntry, ModelStoreModel } from './types';

/** SWR key for the local model list. */
export const LOCAL_MODELS_SWR_KEY = 'local-models';

/** Poll aggressively while bytes are moving so the card tracks the download. */
const DOWNLOADING_POLL_MS = 300;
/** Starting is slower to change; a tighter interval buys little. */
const STARTING_POLL_MS = 1000;
const IDLE_POLL_MS = 8000;

interface LocalModelsQuery {
  models: LocalModelStatus[];
  houmoError?: string;
}

async function fetchLocalModels(): Promise<LocalModelsQuery> {
  const response = await ipcBridge.localModel.list.invoke();
  return {
    models: response?.models ?? [],
    houmoError: response?.houmo_error,
  };
}

function liveFields(status: LocalModelStatus | undefined): Pick<
  ModelStoreModel,
  | 'status'
  | 'downloadProgress'
  | 'downloadSpeed'
  | 'downloadPhase'
  | 'downloadError'
  | 'port'
  | 'baseUrl'
  | 'installPath'
  | 'mainGguf'
  | 'mmproj'
  | 'dflash'
  | 'supportsMtp'
  | 'supportsDflash'
  | 'contextSize'
  | 'maxTokens'
  | 'lastLaunchOptions'
> {
  return {
    status: status?.status ?? 'uninstalled',
    downloadProgress: status?.download_progress ?? 0,
    downloadSpeed: status?.download_speed ?? 0,
    downloadPhase: status?.download_phase,
    downloadError: status?.download_error,
    port: status?.port,
    baseUrl: status?.base_url,
    installPath: status?.install_path,
    mainGguf: status?.main_gguf ?? '',
    mmproj: status?.mmproj,
    dflash: status?.dflash,
    supportsMtp: status?.supports_mtp ?? false,
    supportsDflash: status?.supports_dflash ?? false,
    contextSize: status?.context_size ?? 0,
    maxTokens: status?.max_tokens ?? 0,
    lastLaunchOptions: status?.last_launch_options,
  };
}

function mergeCatalog(statuses: LocalModelStatus[] | undefined): ModelStoreModel[] {
  const byId = new Map((statuses ?? []).map((status) => [status.id, status]));
  const builtin = (catalog as ModelStoreCatalogEntry[]).map((entry) => ({
    ...entry,
    source: 'builtin' as const,
    ...liveFields(byId.get(entry.id)),
  }));
  const houmo = (statuses ?? [])
    .filter((status) => status.source === 'houmo')
    .map((status) => ({
      id: status.id,
      displayName: status.display_name,
      description: status.description,
      type: status.type,
      capabilities: status.capabilities,
      tags: status.tags,
      scenarios: status.scenarios,
      parameterCount: status.parameter_count,
      sizeBytes: status.size_bytes,
      icon: status.icon,
      engines: status.engines,
      npu: status.npu,
      realtime: status.realtime,
      author: status.author,
      license: status.license,
      source: 'houmo' as const,
      ...liveFields(status),
    }));
  return [...builtin, ...houmo];
}

/**
 * Local model list with fast polling while any model is downloading or
 * starting. The provider catalog refreshes only after explicit actions (see
 * [`refreshProviderCaches`]).
 */
export function useLocalModels() {
  const { t } = useTranslation();
  const query = useSWR<LocalModelsQuery>(LOCAL_MODELS_SWR_KEY, fetchLocalModels, {
    revalidateOnFocus: false,
    revalidateOnReconnect: false,
    shouldRetryOnError: false,
    refreshInterval: (latest) => {
      const data = latest?.models ?? [];
      if (data.some((model) => model.status === 'downloading')) return DOWNLOADING_POLL_MS;
      if (data.some((model) => model.status === 'starting')) return STARTING_POLL_MS;
      return IDLE_POLL_MS;
    },
  });

  const models = useMemo(() => mergeCatalog(query.data?.models), [query.data]);
  const notifiedErrors = useRef<Map<string, string>>(new Map());

  useEffect(() => {
    for (const notice of diffDownloadErrors(notifiedErrors.current, models)) {
      Message.error(formatDownloadError(t, notice.error));
    }
  }, [models, t]);

  return {
    models,
    houmoError: query.data?.houmoError,
    isLoading: query.isLoading,
    error: query.error as Error | undefined,
    refresh: query.mutate,
  };
}

/**
 * Refresh `providers` and `models-for-task:` caches after a start/stop/uninstall
 * so the chat model picker reflects the running model.
 */
export async function refreshProviderCaches(): Promise<void> {
  await mutate('providers');
  await mutate(
    (key: unknown) => typeof key === 'string' && key.startsWith('models-for-task:'),
    undefined,
    { revalidate: true }
  );
}
