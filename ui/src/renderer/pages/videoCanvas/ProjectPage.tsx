/**
 * Full open-ai-canvas project workspace (ported), hydrated from allo `/api/video-canvas`.
 */

import React, { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { App as AntApp, ConfigProvider, Spin } from 'antd';
import { Button } from '@arco-design/web-react';
import { QueryClientProvider } from '@tanstack/react-query';
import CanvasProjectPage from '@oc/pages/canvas/project';
import { hydrateCanvasProjectFromServer, syncCanvasProjectToServer } from './lib/ocBridge';
import VimaxProvenanceBar from './lib/VimaxProvenanceBar';
import { syncOcConfigFromAlloMediaModels } from './lib/syncOcModels';
import { videoCanvasQueryClient } from './lib/queryClient';
import { getVideoCanvasAntTheme } from './lib/ocAntTheme';
import { useCanvasStore } from '@oc/stores/canvas/use-canvas-store';
import { useThemeStore } from '@oc/stores/use-theme-store';
import { useUserStore } from '@oc/stores/use-user-store';
import { setActiveUserScope } from '@oc/lib/user-scope';
import { getFeatureAvailability } from '@oc/services/api/auth';
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
  const [error, setError] = useState<string | null>(null);
  const hydrated = useCanvasStore((s) => s.hydrated);
  const colorTheme = useVideoCanvasThemeSync();
  const { whoami, authState } = useCloudAuth();

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
    void getFeatureAvailability()
      .then((payload) => {
        useUserStore.getState().setFeatures(payload.features);
      })
      .catch(() => {
        useUserStore.getState().setFeatures({
          shortDramaEnabled: false,
          taskCenterEnabled: true,
          creditsEnabled: false,
        });
      });
  }, []);

  useEffect(() => {
    if (!canvasId) return;
    let cancelled = false;
    setReady(false);
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
        await waitHydrated();
        await syncOcConfigFromAlloMediaModels().catch((err) => {
          console.warn('[videoCanvas] syncOcConfigFromAlloMediaModels failed', err);
        });
        await hydrateCanvasProjectFromServer(canvasId);
        if (!cancelled) setReady(true);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [canvasId, hydrated]);

  // Persist back to allo server while editing.
  useEffect(() => {
    if (!ready || !canvasId) return;
    const unsub = useCanvasStore.subscribe((state, prev) => {
      const cur = state.projects.find((p) => p.id === canvasId);
      const old = prev.projects.find((p) => p.id === canvasId);
      if (!cur || cur === old) return;
      void syncCanvasProjectToServer(canvasId).catch((err) => {
        console.warn('[videoCanvas] sync failed', err);
      });
    });
    const timer = window.setInterval(() => {
      void syncCanvasProjectToServer(canvasId).catch(() => undefined);
    }, 5000);
    return () => {
      unsub();
      window.clearInterval(timer);
      void syncCanvasProjectToServer(canvasId).catch(() => undefined);
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
      <div className={styles.centerFull}>
        <Spin size='large' />
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
              <CanvasProjectPage />
            </div>
          </AntApp>
        </ConfigProvider>
      </QueryClientProvider>
    </div>
  );
};

export default VideoCanvasProjectPage;
