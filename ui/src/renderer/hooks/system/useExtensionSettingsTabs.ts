import { useCallback, useEffect, useState } from 'react';
import { extensions as extensionsIpc, type IExtensionSettingsTab } from '@/common/adapter/ipcBridge';

export type ExtensionSettingsTabsStatus = 'loading' | 'ready' | 'error';

export type ExtensionSettingsTabsState = {
  tabs: IExtensionSettingsTab[];
  status: ExtensionSettingsTabsStatus;
  error: unknown | null;
};

type ExtensionSettingsTabsSnapshot = ExtensionSettingsTabsState & {
  refresh: () => Promise<void>;
};

let cachedState: ExtensionSettingsTabsState = {
  tabs: [],
  status: 'loading',
  error: null,
};
let initialized = false;
let inflight: Promise<void> | null = null;
const subscribers = new Set<(state: ExtensionSettingsTabsState) => void>();
let stateChangedUnsubscribe: (() => void) | null = null;

function publish(state: ExtensionSettingsTabsState): void {
  cachedState = state;
  for (const listener of subscribers) listener(state);
}

async function refreshTabs(): Promise<void> {
  if (inflight) return inflight;

  publish({ ...cachedState, status: 'loading', error: null });
  inflight = extensionsIpc.getSettingsTabs
    .invoke()
    .then((tabs) => {
      initialized = true;
      publish({ tabs: tabs ?? [], status: 'ready', error: null });
    })
    .catch((error: unknown) => {
      initialized = true;
      console.error('[useExtensionSettingsTabs] Failed to load tabs:', error);
      // Preserve an already rendered navigation list, but make the failure
      // visible to consumers instead of treating an empty list as loading.
      publish({ ...cachedState, status: 'error', error });
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

function ensureStateListener(): void {
  if (stateChangedUnsubscribe) return;
  stateChangedUnsubscribe = extensionsIpc.stateChanged.on(() => {
    void refreshTabs();
  });
}

/**
 * Shared, observable extension settings state. A ready empty array now means
 * "there are no extension settings", while a failed request remains retryable.
 */
export function useExtensionSettingsTabs(): ExtensionSettingsTabsSnapshot {
  const [state, setState] = useState<ExtensionSettingsTabsState>(() => cachedState);

  useEffect(() => {
    subscribers.add(setState);
    ensureStateListener();

    if (!initialized) {
      void refreshTabs();
    } else {
      setState(cachedState);
    }

    return () => {
      subscribers.delete(setState);
    };
  }, []);

  const refresh = useCallback(() => refreshTabs(), []);
  return { ...state, refresh };
}
