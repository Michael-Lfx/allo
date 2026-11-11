/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { ipcBridge } from '@/common';
import type {
  IPresentationCapability,
  IPresentationInstallProgress,
  PresentationInstallStage,
} from '@/common/adapter/ipcBridge';

export type PresentationAction = 'install' | 'repair' | 'cancel' | 'uninstall';

/** Steps shown to the user, in execution order. */
export const PRESENTATION_INSTALL_STAGES: readonly PresentationInstallStage[] = [
  'fetching_skill',
  'preparing_python',
  'installing_dependencies',
  'self_test',
  'committing',
];

/**
 * 0–100 for one stage: byte progress when the download size is known (Python),
 * object progress for the git fetch, otherwise `null` (indeterminate).
 */
export function stagePercent(progress: IPresentationInstallProgress | undefined): number | null {
  if (!progress) return null;
  if (progress.total_bytes > 0) {
    return Math.min(100, Math.round((progress.received_bytes / progress.total_bytes) * 100));
  }
  if (progress.total_objects > 0) {
    return Math.min(100, Math.round((progress.received_objects / progress.total_objects) * 100));
  }
  return null;
}

/** Position of the running stage (1-based) out of the visible stages. */
export function stageIndex(stage: PresentationInstallStage | undefined): number {
  if (!stage) return 0;
  return PRESENTATION_INSTALL_STAGES.indexOf(stage) + 1;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 MB';
  const mb = bytes / (1024 * 1024);
  return mb >= 10 ? `${Math.round(mb)} MB` : `${mb.toFixed(1)} MB`;
}

/**
 * Live view of the instance-wide ppt-master capability: initial `status`,
 * then every `presentationCapability.status` push. Actions resolve once the
 * backend accepted them; the outcome arrives through the same push.
 */
export function usePresentationCapability(enabled = true) {
  const [capability, setCapability] = useState<IPresentationCapability | null>(null);
  const [loading, setLoading] = useState(enabled);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<PresentationAction | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const next = await ipcBridge.presentation.status.invoke();
      if (mounted.current) {
        setCapability(next);
        setError(null);
      }
    } catch (e) {
      if (mounted.current) setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return undefined;
    void refresh();
    return ipcBridge.presentation.onStatus.on((next) => {
      if (mounted.current) setCapability(next);
    });
  }, [enabled, refresh]);

  const run = useCallback(
    async (action: PresentationAction, call: () => Promise<{ capability: IPresentationCapability }>) => {
      setPending(action);
      setError(null);
      try {
        const response = await call();
        if (mounted.current) setCapability(response.capability);
        return true;
      } catch (e) {
        if (mounted.current) setError(e instanceof Error ? e.message : String(e));
        return false;
      } finally {
        if (mounted.current) setPending(null);
      }
    },
    []
  );

  const install = useCallback(
    () => run('install', () => ipcBridge.presentation.install.invoke({ force: false })),
    [run]
  );
  const repair = useCallback(() => run('repair', () => ipcBridge.presentation.install.invoke({ force: true })), [run]);
  const cancel = useCallback(() => run('cancel', () => ipcBridge.presentation.cancel.invoke()), [run]);
  const uninstall = useCallback(() => run('uninstall', () => ipcBridge.presentation.uninstall.invoke()), [run]);

  return { capability, loading, error, pending, refresh, install, repair, cancel, uninstall };
}
