/**
 * Full open-ai-canvas project workspace (ported), hydrated from allo `/api/video-canvas`.
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { App as AntApp, ConfigProvider, Spin } from 'antd';
import { Button } from '@arco-design/web-react';
import { QueryClientProvider } from '@tanstack/react-query';
import CanvasProjectPage from '@oc/pages/canvas/project';
import { hydrateCanvasProjectFromServer, syncCanvasProjectToServer } from './lib/ocBridge';
import { getCanvasProject } from './api';
import { createCanvasProjectAutosaveController } from './lib/canvasProjectAutosave';
import VimaxProvenanceBar from './lib/VimaxProvenanceBar';
import { syncOcConfigFromAlloMediaModels } from './lib/syncOcModels';
import { videoCanvasQueryClient } from './lib/queryClient';
import { getVideoCanvasAntTheme } from './lib/ocAntTheme';
import { useCanvasStore } from '@oc/stores/canvas/use-canvas-store';
import { useThemeStore } from '@oc/stores/use-theme-store';
import { useUserStore } from '@oc/stores/use-user-store';
import { setActiveUserScope } from '@oc/lib/user-scope';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import styles from './index.module.css';

// Ant Design styles for the ported open-ai-canvas workspace.
import 'antd/dist/reset.css';
import '@oc/styles/globals.css';
import '@oc/components/video-player.css';

function useVideoCanvasThemeSync() {
  const theme = useThemeStore((s) => s.theme);
  useEffect(() => {
    const root = document.documentElement;
    const prevDark = root.classList.contains('dark');
    const prevScheme = root.style.colorScheme;
    const apply = (next: 'light' | 'dark') => {
      root.classList.toggle('dark', next === 'dark');
      root.style.colorScheme = next;
    };
    apply(theme);
    return () => {
      root.classList.toggle('dark', prevDark);
      root.style.colorScheme = prevScheme;
    };
  }, [theme]);
  return theme;
}

const VideoCanvasProjectPage: React.FC = () => {
  const { projectId, id } = useParams<{ projectId?: string; id?: string }>();
  const canvasId = projectId || id || '';
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [ready, setReady] = useState(false);
  const [modelCatalogReady, setModelCatalogReady] = useState(false);
  const [modelCatalogFailed, setModelCatalogFailed] = useState(false);
  const [catalogRetrying, setCatalogRetrying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const colorTheme = useVideoCanvasThemeSync();
  const { whoami, authState } = useCloudAuth();
  const catalogSyncGeneration = useRef(0);

  const syncModelCatalog = useCallback(async (generation: number) => {
    try {
      await syncOcConfigFromAlloMediaModels();
      if (catalogSyncGeneration.current !== generation) return;
      setModelCatalogReady(true);
      setModelCatalogFailed(false);
    } catch (err) {
      console.warn('[videoCanvas] syncOcConfigFromAlloMediaModels failed', err);
      if (catalogSyncGeneration.current !== generation) return;
      setModelCatalogReady(false);
      setModelCatalogFailed(true);
    }
  }, []);

  const retryModelCatalog = useCallback(() => {
    if (catalogRetrying) return;
    const generation = catalogSyncGeneration.current;
    setCatalogRetrying(true);
    void syncModelCatalog(generation).finally(() => {
      if (catalogSyncGeneration.current === generation) setCatalogRetrying(false);
    });
  }, [catalogRetrying, syncModelCatalog]);

  useEffect(() => {
    const accountId =
      (authState.phase === 'authenticated' && authState.accountId) ||
      whoami?.userId ||
      whoami?.email ||
      whoami?.username ||
      null;
    setActiveUserScope(accountId);
    if (accountId) {
      useUserStore.getState().setUser({
        id: accountId,
        username: whoami?.username || accountId,
        email: whoami?.email,
        displayName: whoami?.username || whoami?.email || accountId,
        role: 'user',
        status: 'active',
      });
    }
  }, [authState, whoami]);

  useEffect(() => {
    if (!canvasId) return;
    let cancelled = false;
    const generation = catalogSyncGeneration.current + 1;
    catalogSyncGeneration.current = generation;
    setReady(false);
    setModelCatalogReady(false);
    setModelCatalogFailed(false);
    setCatalogRetrying(false);
    setError(null);
    void (async () => {
      try {
        // Wait for zustand persist hydrate
        const waitHydrated = () =>
          new Promise<void>((resolve) => {
            if (useCanvasStore.getState().hydrated) {
              resolve();
              return;
            }
            const unsub = useCanvasStore.subscribe((s) => {
              if (s.hydrated) {
                unsub();
                resolve();
              }
            });
          });
        // Overlap network fetch with local persist hydrate — the document is
        // needed to render the workspace. Models are only needed when a
        // generation control is used, so do not hold the canvas behind an
        // unavailable or slow catalog.
        const fetchPromise = getCanvasProject(canvasId);
        await waitHydrated();
        await hydrateCanvasProjectFromServer(canvasId, await fetchPromise);
        if (cancelled) return;
        setReady(true);
        void syncModelCatalog(generation);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
      catalogSyncGeneration.current += 1;
    };
  }, [canvasId, syncModelCatalog]);

  // Persist back to allo server while editing.
  useEffect(() => {
    if (!ready || !canvasId) return;
    const autosave = createCanvasProjectAutosaveController({
      save: () => syncCanvasProjectToServer(canvasId),
      onError: (err, retryDelayMs) => {
        console.warn(`[videoCanvas] sync failed; retrying in ${retryDelayMs}ms`, err);
      },
    });

    const unsub = useCanvasStore.subscribe((state, prev) => {
      const cur = state.projects.find((p) => p.id === canvasId);
      const old = prev.projects.find((p) => p.id === canvasId);
      if (!cur || cur === old) return;
      autosave.markDirty();
    });
    const flushOnPageHide = () => {
      void autosave.flush();
    };
    const flushWhenHidden = () => {
      if (document.visibilityState === 'hidden') flushOnPageHide();
    };
    window.addEventListener('pagehide', flushOnPageHide);
    document.addEventListener('visibilitychange', flushWhenHidden);
    return () => {
      unsub();
      window.removeEventListener('pagehide', flushOnPageHide);
      document.removeEventListener('visibilitychange', flushWhenHidden);
      void autosave.dispose();
    };
  }, [ready, canvasId]);

  if (error) {
    return (
      <div className={styles.centerFull}>
        <p>{error}</p>
        <Button onClick={() => navigate('/video-generation?mode=creation')}>
          {t('videoCanvas.project.back', { defaultValue: '返回' })}
        </Button>
      </div>
    );
  }

  if (!ready || !canvasId) {
    return (
      <div className={styles.centerFull} role='status' aria-live='polite'>
        <Spin size='large' />
        <p className={styles.loadingHint}>
          {t('videoCanvas.project.loading', { defaultValue: '正在加载画布…' })}
        </p>
      </div>
    );
  }

  return (
    <div className={`${styles.ocShell}${colorTheme === 'dark' ? ' dark' : ''}`}>
      <QueryClientProvider client={videoCanvasQueryClient}>
        <ConfigProvider theme={getVideoCanvasAntTheme(colorTheme === 'dark')}>
          <AntApp>
            <div style={{ position: 'relative', width: '100%', height: '100%' }}>
              <VimaxProvenanceBar projectId={canvasId} />
              {modelCatalogFailed ? (
                <div className={styles.catalogBanner} role='status'>
                  <span>
                    {t('videoCanvas.project.catalogFailed', {
                      defaultValue: '模型目录加载失败，生成任务暂缓，避免用空配置提交。',
                    })}
                  </span>
                  <Button size='mini' loading={catalogRetrying} onClick={retryModelCatalog}>
                    {t('videoCanvas.project.catalogRetry', { defaultValue: '重试' })}
                  </Button>
                </div>
              ) : null}
              <CanvasProjectPage modelCatalogReady={modelCatalogReady} />
            </div>
          </AntApp>
        </ConfigProvider>
      </QueryClientProvider>
    </div>
  );
};

export default VideoCanvasProjectPage;
