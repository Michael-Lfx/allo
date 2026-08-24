import React, { createContext, useCallback, useContext, useMemo, useRef, useState } from 'react';
import { ipcBridge } from '@/common';
import type { ICloudWhoami } from '@/common/adapter/ipcBridge';
import { CLOUD_AUTH_EXPIRED_EVENT, isBackendHttpError, isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { configService } from '@/common/config/configService';
import { getBrowserStorageGeneration } from '@/common/utils/browserStorageKey';
import CloudSessionExpiredModal from '@renderer/components/auth/CloudSessionExpiredModal';
import {
  clearAvailableModelsCache,
  refreshProvidersCatalog,
  setProviderCatalogContext,
  type ProviderCatalogRefreshResult,
} from '@renderer/hooks/agent/useModelProviderList';
import {
  classifyCloudModelEnvironment,
  type CloudModelEnvironmentClassification,
} from './cloudModelEnvironment';
import { useAuth } from './AuthContext';

export type CloudAuthState =
  | { phase: 'unknown' }
  | { phase: 'authenticated'; accountId: string }
  | { phase: 'unauthenticated' }
  | {
      phase: 'offline';
      previousAccountId?: string;
      reason: 'network' | 'server_error';
    };

/**
 * Compatibility status for existing route guards. New code should prefer
 * `authState`; offline keeps the previous authenticated status when a session
 * was already confirmed so a transient cloud outage cannot redirect the user
 * to login or clear local model state.
 */
export type CloudAuthStatus = 'checking' | 'authenticated' | 'unauthenticated';
export type CloudAuthRefreshResult = 'authenticated' | 'unauthenticated' | 'offline' | 'stale';
export type CloudModelStatus = 'idle' | 'restoring' | 'ready' | 'degraded' | 'failed';

export type ModelEnvironmentState = {
  phase: 'restoring' | 'ready' | 'degraded';
  reason?: 'using_cached_catalog' | 'empty_catalog' | 'sync_failed' | 'resolver_failed';
  usableModelCount: number;
  canRetry: boolean;
  error?: Error;
};

interface CloudAuthContextValue {
  ready: boolean;
  authState: CloudAuthState;
  /** @deprecated Use authState.phase and its account identity. */
  status: CloudAuthStatus;
  whoami: ICloudWhoami | null;
  modelEnvironment: ModelEnvironmentState;
  /** @deprecated Use modelEnvironment. Kept for existing selectors during migration. */
  modelStatus: CloudModelStatus;
  modelError: Error | null;
  refresh: (options?: { forceModelSync?: boolean }) => Promise<CloudAuthRefreshResult>;
  retryModelEnvironment: () => Promise<void>;
  logout: () => Promise<void>;
}

const CloudAuthContext = createContext<CloudAuthContextValue | undefined>(undefined);

const emptyModelEnvironment = (): ModelEnvironmentState => ({
  phase: 'restoring',
  usableModelCount: 0,
  canRetry: true,
});

const getStorageGenerationOrUndefined = (): string | undefined => {
  try {
    return getBrowserStorageGeneration();
  } catch {
    return undefined;
  }
};

const accountIdForProfile = (profile: ICloudWhoami): string =>
  profile.userId || profile.email || profile.username || 'authenticated-account';

const normalizeError = (error: unknown): Error => (error instanceof Error ? error : new Error(String(error)));

const offlineReasonForError = (error: unknown): 'network' | 'server_error' => {
  if (isBackendHttpError(error) && (error.status === 403 || error.status >= 500)) return 'server_error';
  return 'network';
};

export const CloudAuthProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
  const { status: localStatus, ready: localReady } = useAuth();
  const [authState, setAuthState] = useState<CloudAuthState>({ phase: 'unknown' });
  const [whoami, setWhoami] = useState<ICloudWhoami | null>(null);
  const [ready, setReady] = useState(false);
  const [modelStatus, setModelStatus] = useState<CloudModelStatus>('idle');
  const [modelEnvironment, setModelEnvironment] = useState<ModelEnvironmentState>(emptyModelEnvironment);
  const [modelError, setModelError] = useState<Error | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const restoreRunRef = useRef(0);
  const authStateRef = useRef(authState);
  const whoamiRef = useRef(whoami);
  const storageGenerationRef = useRef<string | undefined>(undefined);
  const accountIdRef = useRef<string | undefined>(undefined);
  const expiredModalOpenRef = useRef(false);
  const [expiredModalOpen, setExpiredModalOpen] = useState(false);
  authStateRef.current = authState;
  whoamiRef.current = whoami;

  const isCurrentRun = useCallback((runId: number, controller: AbortController): boolean => {
    return !controller.signal.aborted && runId === restoreRunRef.current;
  }, []);

  const beginRun = useCallback(() => {
    const runId = ++restoreRunRef.current;
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    return { runId, controller };
  }, []);

  const restoreModelEnvironment = useCallback(
    async (
      runId: number,
      controller: AbortController,
      accountId: string,
      force: boolean
    ): Promise<
      | (ProviderCatalogRefreshResult &
          CloudModelEnvironmentClassification & { usableModelCount: number; resolveError?: Error })
      | null
    > => {
      const storageGeneration = getStorageGenerationOrUndefined();
      setProviderCatalogContext(accountId, storageGeneration);
      const refreshResult = await refreshProvidersCatalog({
        force,
        accountId,
        storageGeneration,
      });
      if (!isCurrentRun(runId, controller) || refreshResult.stale) return null;

      let resolvedModelCount = 0;
      let resolveError: Error | undefined;
      try {
        const resolved = await ipcBridge.modelProfile.resolve.invoke({ task: 'chat' });
        if (!isCurrentRun(runId, controller)) return null;
        resolvedModelCount = resolved?.models?.length ?? 0;
        const defaultModel = configService.get('nomi.defaultModel');
        if (defaultModel) {
          const defaultModelAvailable = (resolved?.models ?? []).some(
            (model) =>
              String(model.provider_id) === String(defaultModel.provider_id) && model.model === defaultModel.model
          );
          if (!defaultModelAvailable) {
            // Preserve the explicit preference. A replacement is written only
            // from an explicit user selection, never during recovery.
            console.warn('[cloud-models] persisted default model is not in the restored catalog', defaultModel);
          }
        }
      } catch (error) {
        if (!isCurrentRun(runId, controller)) return null;
        resolveError = normalizeError(error);
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
      return { ...refreshResult, ...classification, usableModelCount: resolvedModelCount, resolveError };
    },
    [isCurrentRun]
  );

  const restoreModelEnvironmentForRun = useCallback(
    async (runId: number, controller: AbortController, accountId: string, force: boolean): Promise<void> => {
      if (!isCurrentRun(runId, controller)) return;
      setModelStatus('restoring');
      setModelEnvironment({ phase: 'restoring', usableModelCount: 0, canRetry: true });
      setModelError(null);
      try {
        const result = await restoreModelEnvironment(runId, controller, accountId, force);
        if (!result || !isCurrentRun(runId, controller)) return;
        const nextError = result.error ?? null;
        if (result.status === 'failed') {
          setModelStatus('failed');
          setModelEnvironment({
            phase: 'degraded',
            reason: result.resolveError ? 'resolver_failed' : 'empty_catalog',
            usableModelCount: result.usableModelCount,
            canRetry: true,
            error: nextError ?? undefined,
          });
        } else {
          setModelStatus(result.status);
          setModelEnvironment({
            phase: result.status,
            reason: result.status === 'degraded' ? 'sync_failed' : undefined,
            usableModelCount: result.usableModelCount,
            canRetry: true,
            error: nextError ?? undefined,
          });
        }
        setModelError(nextError);
      } catch (error) {
        if (!isCurrentRun(runId, controller)) return;
        const normalized = normalizeError(error);
        setModelStatus('failed');
        setModelEnvironment({
          phase: 'degraded',
          reason: 'resolver_failed',
          usableModelCount: 0,
          canRetry: true,
          error: normalized,
        });
        setModelError(normalized);
      }
    },
    [isCurrentRun, restoreModelEnvironment]
  );

  const refresh = useCallback(
    async (options: { forceModelSync?: boolean } = {}): Promise<CloudAuthRefreshResult> => {
      const { runId, controller } = beginRun();
      const forceModelSync = options.forceModelSync === true;
      if (!localReady || localStatus !== 'authenticated') {
        await clearAvailableModelsCache();
        if (!isCurrentRun(runId, controller)) return 'stale';
        setAuthState({ phase: 'unauthenticated' });
        setWhoami(null);
        accountIdRef.current = undefined;
        setModelStatus('idle');
        setModelEnvironment({ phase: 'degraded', reason: 'empty_catalog', usableModelCount: 0, canRetry: false });
        setModelError(null);
        setReady(localReady);
        return 'unauthenticated';
      }

      if (!isCurrentRun(runId, controller)) return 'stale';
      setAuthState({ phase: 'unknown' });

      try {
        const profile = await ipcBridge.cloud.whoami.invoke();
        if (!isCurrentRun(runId, controller)) return 'stale';
        if (!profile.authenticated) {
          await clearAvailableModelsCache();
          if (!isCurrentRun(runId, controller)) return 'stale';
          setAuthState({ phase: 'unauthenticated' });
          setWhoami(null);
          accountIdRef.current = undefined;
          setModelStatus('idle');
          setModelEnvironment({ phase: 'degraded', reason: 'empty_catalog', usableModelCount: 0, canRetry: false });
          setModelError(null);
          return 'unauthenticated';
        }

        const accountId = accountIdForProfile(profile);
        const storageGeneration = getStorageGenerationOrUndefined();
        const accountChanged = !!accountIdRef.current && accountIdRef.current !== accountId;
        const generationChanged =
          !!storageGenerationRef.current && storageGenerationRef.current !== storageGeneration;
        setProviderCatalogContext(accountId, storageGeneration);
        if (accountChanged || generationChanged) {
          await clearAvailableModelsCache();
          if (!isCurrentRun(runId, controller)) return 'stale';
        }
        accountIdRef.current = accountId;
        storageGenerationRef.current = storageGeneration;
        setWhoami(profile);
        setAuthState({ phase: 'authenticated', accountId });
        await restoreModelEnvironmentForRun(runId, controller, accountId, forceModelSync);
        return isCurrentRun(runId, controller) ? 'authenticated' : 'stale';
      } catch (error) {
        if (!isCurrentRun(runId, controller)) return 'stale';
        console.error('Failed to fetch cloud auth status:', error);
        const previousAccountId = accountIdRef.current;
        if (isInvalidCloudSessionError(error)) {
          await clearAvailableModelsCache();
          if (!isCurrentRun(runId, controller)) return 'stale';
          setAuthState({ phase: 'unauthenticated' });
          setWhoami(null);
          accountIdRef.current = undefined;
          setModelStatus('idle');
          setModelEnvironment({ phase: 'degraded', reason: 'empty_catalog', usableModelCount: 0, canRetry: false });
          setModelError(null);
          return 'unauthenticated';
        } else {
          const reason = offlineReasonForError(error);
          setAuthState({ phase: 'offline', previousAccountId, reason });
          // Keep the last confirmed profile and model environment. A network
          // outage must not turn a usable local catalog into a login reset.
          setModelError(normalizeError(error));
          return 'offline';
        }
      } finally {
        if (isCurrentRun(runId, controller)) setReady(true);
      }
    },
    [beginRun, isCurrentRun, localReady, localStatus, restoreModelEnvironmentForRun]
  );

  React.useEffect(() => {
    void refresh();
    return () => {
      abortRef.current?.abort();
    };
  }, [refresh]);

  const closeExpiredModal = useCallback(() => {
    expiredModalOpenRef.current = false;
    setExpiredModalOpen(false);
  }, []);

  const goCloudRelogin = useCallback(() => {
    closeExpiredModal();
    try {
      if (typeof window === 'undefined') return;
      if (window.location.hash.includes('/cloud-login')) return;
      window.location.hash = '/cloud-login';
    } catch {
      // Hash navigation is best-effort; the logout still cleared the session.
    }
  }, [closeExpiredModal]);

  const logout = useCallback(async () => {
    // Invalidate any in-flight whoami/catalog run before changing the server
    // session. A late response must never repopulate the renderer after logout.
    const { runId, controller } = beginRun();
    try {
      await ipcBridge.cloud.logout.invoke();
    } catch (error) {
      console.error('Cloud logout failed:', error);
    } finally {
      await clearAvailableModelsCache();
      if (!isCurrentRun(runId, controller)) return;
      setAuthState({ phase: 'unauthenticated' });
      setWhoami(null);
      accountIdRef.current = undefined;
      setModelStatus('idle');
      setModelEnvironment({ phase: 'degraded', reason: 'empty_catalog', usableModelCount: 0, canRetry: false });
      setModelError(null);
      setReady(true);
    }
  }, [beginRun, isCurrentRun]);

  React.useEffect(() => {
    if (typeof window === 'undefined') return undefined;
    const onCloudAuthExpired = () => {
      try {
        const hash = window.location.hash ?? '';
        if (hash.includes('/cloud-login') || hash.includes('/login')) return;
      } catch {
        // Continue and show the modal if location is unavailable.
      }
      if (expiredModalOpenRef.current) return;
      expiredModalOpenRef.current = true;
      setExpiredModalOpen(true);
      void logout();
    };
    window.addEventListener(CLOUD_AUTH_EXPIRED_EVENT, onCloudAuthExpired);
    return () => window.removeEventListener(CLOUD_AUTH_EXPIRED_EVENT, onCloudAuthExpired);
  }, [logout]);

  React.useEffect(() => {
    if (authState.phase === 'authenticated') closeExpiredModal();
  }, [authState.phase, closeExpiredModal]);

  const retryModelEnvironment = useCallback(async () => {
    const accountId = accountIdRef.current;
    if (!accountId || authStateRef.current.phase !== 'authenticated') {
      await refresh({ forceModelSync: true });
      return;
    }
    const { runId, controller } = beginRun();
    await restoreModelEnvironmentForRun(runId, controller, accountId, true);
  }, [beginRun, refresh, restoreModelEnvironmentForRun]);

  const status: CloudAuthStatus =
    authState.phase === 'authenticated' || (authState.phase === 'offline' && !!authState.previousAccountId)
      ? 'authenticated'
      : authState.phase === 'unauthenticated'
        ? 'unauthenticated'
        : 'checking';

  const value = useMemo<CloudAuthContextValue>(
    () => ({
      ready,
      authState,
      status,
      whoami,
      modelEnvironment,
      modelStatus,
      modelError,
      refresh,
      retryModelEnvironment,
      logout,
    }),
    [authState, logout, modelEnvironment, modelError, modelStatus, ready, refresh, retryModelEnvironment, status, whoami]
  );

  return (
    <CloudAuthContext.Provider value={value}>
      {children}
      <CloudSessionExpiredModal
        visible={expiredModalOpen}
        onCancel={closeExpiredModal}
        onRelogin={goCloudRelogin}
      />
    </CloudAuthContext.Provider>
  );
};

export function useCloudAuth(): CloudAuthContextValue {
  const context = useContext(CloudAuthContext);
  if (!context) {
    throw new Error('useCloudAuth must be used within a CloudAuthProvider');
  }
  return context;
}
