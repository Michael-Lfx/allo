import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { mutate } from 'swr';
import { ipcBridge } from '@/common';
import type { ICloudWhoami } from '@/common/adapter/ipcBridge';
import { fetchProviders, PROVIDERS_SWR_KEY } from '@renderer/hooks/agent/useModelProviderList';
import { useAuth } from './AuthContext';

export type CloudAuthStatus = 'checking' | 'authenticated' | 'unauthenticated';

interface CloudAuthContextValue {
  ready: boolean;
  status: CloudAuthStatus;
  whoami: ICloudWhoami | null;
  refresh: () => Promise<void>;
  logout: () => Promise<void>;
}

const CloudAuthContext = createContext<CloudAuthContextValue | undefined>(undefined);

export const CloudAuthProvider: React.FC<React.PropsWithChildren> = ({ children }) => {
  const { status: localStatus, ready: localReady } = useAuth();
  const [status, setStatus] = useState<CloudAuthStatus>('checking');
  const [whoami, setWhoami] = useState<ICloudWhoami | null>(null);
  const [ready, setReady] = useState(false);
  const abortRef = useRef<AbortController | null>(null);

  const refresh = useCallback(async () => {
    if (!localReady || localStatus !== 'authenticated') {
      setStatus('checking');
      setWhoami(null);
      setReady(localReady);
      return;
    }

    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    setStatus('checking');

    try {
      const profile = await ipcBridge.cloud.whoami.invoke();
      if (controller.signal.aborted) return;
      setWhoami(profile);
      setStatus(profile.authenticated ? 'authenticated' : 'unauthenticated');
      if (profile.authenticated) {
        void mutate(PROVIDERS_SWR_KEY, fetchProviders(), { revalidate: true });
      }
    } catch (error) {
      if (controller.signal.aborted) return;
      console.error('Failed to fetch cloud auth status:', error);
      setWhoami(null);
      setStatus('unauthenticated');
    } finally {
      if (!controller.signal.aborted) {
        setReady(true);
      }
    }
  }, [localReady, localStatus]);

  useEffect(() => {
    void refresh();
    return () => {
      abortRef.current?.abort();
    };
  }, [refresh]);

  const logout = useCallback(async () => {
    try {
      await ipcBridge.cloud.logout.invoke();
    } catch (error) {
      console.error('Cloud logout failed:', error);
    } finally {
      await refresh();
    }
  }, [refresh]);

  const value = useMemo<CloudAuthContextValue>(
    () => ({
      ready,
      status,
      whoami,
      refresh,
      logout,
    }),
    [ready, status, whoami, refresh, logout]
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
