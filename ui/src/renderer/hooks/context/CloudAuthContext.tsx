import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { ipcBridge } from '@/common';
import type { ICloudWhoami } from '@/common/adapter/ipcBridge';
import { configService } from '@/common/config/configService';
import {
  clearAvailableModelsCache,
  refreshProvidersCatalogIfStale,
  type ProviderCatalogRefreshResult,
} from '@renderer/hooks/agent/useModelProviderList';
import {
  classifyCloudModelEnvironment,
  type CloudModelEnvironmentClassification,
} from './cloudModelEnvironment';
import { useAuth } from './AuthContext';

export type CloudAuthStatus = 'checking' | 'authenticated' | 'unauthenticated';
export type CloudModelStatus = 'idle' | 'restoring' | 'ready' | 'degraded' | 'failed';

interface CloudAuthContextValue {
  ready: boolean;
  status: CloudAuthStatus;
  whoami: ICloudWhoami | null;
  modelStatus: CloudModelStatus;
  modelError: Error | null;
  refresh: () => Promise<void>;
  retryModelEnvironment: () => Promise<void>;
  logout: () => Promise<void>;
}

const CloudAuthContext = createContext<CloudAuthContextValue | undefined>(undefined);

export const CloudAuthProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
  const { status: localStatus, ready: localReady } = useAuth();
  const [status, setStatus] = useState<CloudAuthStatus>('checking');
  const [whoami, setWhoami] = useState<ICloudWhoami | null>(null);
  const [ready, setReady] = useState(false);
  const [modelStatus, setModelStatus] = useState<CloudModelStatus>('idle');
  const [modelError, setModelError] = useState<Error | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const restoreRunRef = useRef(0);

  const restoreModelEnvironment = useCallback(async (
    runId: number,
    controller: AbortController,
  ): Promise<(ProviderCatalogRefreshResult & CloudModelEnvironmentClassification) | null> => {
    let refreshResult = await refreshProvidersCatalogIfStale();
    if (controller.signal.aborted || runId !== restoreRunRef.current) return null;
    if (refreshResult.stale) {
      refreshResult = await refreshProvidersCatalogIfStale();
      if (controller.signal.aborted || runId !== restoreRunRef.current) return null;
    }
    let resolvedModelCount = 0;
    let resolveError: Error | undefined;
    try {
      const resolved = await ipcBridge.modelProfile.resolve.invoke({ task: 'chat' });
      if (controller.signal.aborted || runId !== restoreRunRef.current) return null;
      resolvedModelCount = resolved?.models?.length ?? 0;
      const defaultModel = configService.get('nomi.defaultModel');
      if (defaultModel) {
        const defaultModelAvailable = (resolved?.models ?? []).some(
          (model) => String(model.provider_id) === String(defaultModel.provider_id) && model.model === defaultModel.model
        );
        if (!defaultModelAvailable) {
          // Keep the explicit user preference intact, but surface the stale
          // reference in diagnostics. Selectors already render unavailable
          // persisted choices until the user chooses a replacement.
          console.warn('[cloud-models] persisted default model is not in the restored catalog', defaultModel);
        }
      }
    } catch (error) {
      resolveError = error instanceof Error ? error : new Error(String(error));
    }

    const cachedProviderModelCount = refreshResult.providers.reduce((count, provider) => {
      if (provider.enabled === false) return count;
      return count + (provider.models ?? []).filter((model) => provider.model_enabled?.[model] !== false).length;
    }, 0);
    const classification = classifyCloudModelEnvironment({
      resolvedModelCount,
      cachedProviderModelCount,
      syncError: refreshResult.syncError,
      resolveError,
    });
    if (classification.status === 'failed') {
      throw classification.error ?? new Error('No usable cloud models are available');
    }
    return { ...refreshResult, ...classification };
  }, []);

  const restoreModelEnvironmentForRun = useCallback(
    async (runId: number, controller: AbortController): Promise<void> => {
      if (controller.signal.aborted || runId !== restoreRunRef.current) return;
      setModelStatus('restoring');
      setModelError(null);
      try {
        const result = await restoreModelEnvironment(runId, controller);
        if (!result || controller.signal.aborted || runId !== restoreRunRef.current) return;
        setModelStatus(result.status);
        setModelError(result.error ?? null);
      } catch (error) {
        if (controller.signal.aborted || runId !== restoreRunRef.current) return;
        const normalized = error instanceof Error ? error : new Error(String(error));
        setModelStatus('failed');
        setModelError(normalized);
      }
    },
    [restoreModelEnvironment]
  );

  const refresh = useCallback(async () => {
    const runId = ++restoreRunRef.current;
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    if (!localReady || localStatus !== 'authenticated') {
      await clearAvailableModelsCache();
      if (controller.signal.aborted || runId !== restoreRunRef.current) return;
      setStatus('checking');
      setWhoami(null);
      setModelStatus('idle');
      setModelError(null);
      setReady(localReady);
      return;
    }

    if (controller.signal.aborted || runId !== restoreRunRef.current) return;
    setStatus('checking');

    try {
      const profile = await ipcBridge.cloud.whoami.invoke();
      if (controller.signal.aborted || runId !== restoreRunRef.current) return;
      setWhoami(profile);
      setStatus(profile.authenticated ? 'authenticated' : 'unauthenticated');
      if (profile.authenticated) {
        await restoreModelEnvironmentForRun(runId, controller);
      } else {
        await clearAvailableModelsCache();
        if (controller.signal.aborted || runId !== restoreRunRef.current) return;
        setModelStatus('idle');
        setModelError(null);
      }
    } catch (error) {
      if (controller.signal.aborted || runId !== restoreRunRef.current) return;
      console.error('Failed to fetch cloud auth status:', error);
      await clearAvailableModelsCache();
      if (controller.signal.aborted || runId !== restoreRunRef.current) return;
      setWhoami(null);
      setStatus('unauthenticated');
      setModelStatus('idle');
      setModelError(null);
    } finally {
      if (!controller.signal.aborted && runId === restoreRunRef.current) {
        setReady(true);
      }
    }
  }, [localReady, localStatus, restoreModelEnvironmentForRun]);

  useEffect(() => {
    void refresh();
    return () => {
      abortRef.current?.abort();
    };
  }, [refresh]);

  const logout = useCallback(async () => {
    // Invalidate any in-flight whoami/catalog run before changing the server
    // session. Otherwise a late authenticated response can repopulate the
    // renderer after logout has already started clearing it.
    ++restoreRunRef.current;
    abortRef.current?.abort();
    try {
      await ipcBridge.cloud.logout.invoke();
    } catch (error) {
      console.error('Cloud logout failed:', error);
    } finally {
      await refresh();
    }
  }, [refresh]);

  const retryModelEnvironment = useCallback(async () => {
    // Keep the authenticated state and its gate in place while retrying. A
    // full auth refresh briefly changes status to `checking`, which used to
    // render the main UI during the retry and allowed chat selectors to issue
    // requests against the still-empty catalog. The retry contract is exactly
    // sync -> provider refresh -> task resolver -> default-model validation.
    if (status !== 'authenticated' || !whoami?.authenticated) {
      await refresh();
      return;
    }
    const runId = ++restoreRunRef.current;
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    await restoreModelEnvironmentForRun(runId, controller);
  }, [refresh, restoreModelEnvironmentForRun, status, whoami?.authenticated]);

  const value = useMemo<CloudAuthContextValue>(
    () => ({
      ready,
      status,
      whoami,
      modelStatus,
      modelError,
      refresh,
      retryModelEnvironment,
      logout,
    }),
    [ready, status, whoami, modelStatus, modelError, refresh, retryModelEnvironment, logout]
  );

  return <CloudAuthContext.Provider value={value}>{children}</CloudAuthContext.Provider>;
};

export function useCloudAuth(): CloudAuthContextValue {
  const context = useContext(CloudAuthContext);
  if (!context) {
    throw new Error('useCloudAuth must be used within a CloudAuthProvider');
  }
  return context;
}
