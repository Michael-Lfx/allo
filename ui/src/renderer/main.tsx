

// Runtime patches must be imported early
import './utils/ui/runtimePatches';

// Browser adapter setup
import '@/common/adapter/browser';

// React and core dependencies
import type { PropsWithChildren } from 'react';
import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';

// Context providers
import { AuthProvider } from './hooks/context/AuthContext';
import { CloudAuthProvider } from './hooks/context/CloudAuthContext';
import { CreditsProvider } from './hooks/context/CreditsContext';
import { FeedbackProvider } from './hooks/context/FeedbackContext';
import { ThemeProvider } from './hooks/context/ThemeContext';
import { SupportChatProvider } from './features/supportChat/SupportChatProvider';

// Arco Design
import { Alert, Button, ConfigProvider } from '@arco-design/web-react';
// Configure Arco Design to use React 18's createRoot, fixing Message component's CopyReactDOM.render error
import '@arco-design/web-react/es/_util/react-19-adapter';
import '@arco-design/web-react/dist/css/arco.css';
import enUS from '@arco-design/web-react/es/locale/en-US';
import zhCN from '@arco-design/web-react/es/locale/zh-CN';
import { useTranslation } from 'react-i18next';

// Styles
import 'uno.css';
import './styles/arco-override.css';
import '@/renderer/components/chat/SendBox/sendbox.css';
import './styles/themes/index.css';

// Config service — kick off initialization before i18n / theme modules load,
// so their startup paths (which await configService.whenReady()) observe the
// authoritative settings from the backend instead of the empty cache.
import { configService } from '@/common/config/configService';
import { application } from '@/common/adapter/ipcBridge';
import { isHandledAuthExpiredHttpError } from '@/common/adapter/httpBridge';
import { getBrowserStorageGeneration, setBrowserStorageGeneration } from '@/common/utils/browserStorageKey';
configService.initialize().catch((err) => {
  console.error('Failed to initialize config:', err);
});

// i18n
import './services/i18n';
import { registerPwa } from './services/registerPwa';

import { mutate as swrMutate } from 'swr';
import { DETECTED_AGENTS_SWR_KEY, fetchDetectedAgents } from './utils/model/agentTypes';
import { repairAllCronJobTimeZonesOnce } from '@renderer/pages/cron/repairCronJobTimeZone';

// Components and utilities
import AppLoader from './components/layout/AppLoader';
import { maybeTrackRetention } from './utils/analytics/productFunnel';
import Layout from './components/layout/Layout';
import RouteErrorBoundary from './components/layout/RouteErrorBoundary';
import Router from './components/layout/Router';
import Sider from './components/layout/Sider';
import { refreshDetectedAgentsIfStale } from './hooks/agent/useAgents';
import { clearAvailableModelsCache } from './hooks/agent/useModelProviderList';
import {
  shouldScheduleAgentRefreshAfterHashChange,
  shouldScheduleAgentRefreshForHash,
} from './hooks/agent/agentDetectionRefresh';
import { useAuth } from './hooks/context/AuthContext';
import { useCloudAuth } from './hooks/context/CloudAuthContext';
import { ConversationHistoryProvider } from './hooks/context/ConversationHistoryContext';
import HOC from './utils/ui/HOC';

const arcoLocales: Record<string, typeof enUS> = {
  'zh-CN': zhCN,
  'en-US': enUS,
};

const AppProviders: React.FC<PropsWithChildren> = ({ children }) =>
  React.createElement(
    AuthProvider,
    null,
    React.createElement(
      CloudAuthProvider,
      null,
      React.createElement(
        CreditsProvider,
        null,
        React.createElement(
          SupportChatProvider,
        null,
        React.createElement(ThemeProvider, null, React.createElement(FeedbackProvider, null, children))
        )
      )
    )
  );

const Config: React.FC<PropsWithChildren> = ({ children }) => {
  const {
    i18n: { language },
  } = useTranslation();
  const arcoLocale = arcoLocales[language] ?? enUS;

  return React.createElement(ConfigProvider, { theme: { primaryColor: '#4E5969' }, locale: arcoLocale }, children);
};

const CloudModelEnvironmentGate: React.FC<PropsWithChildren> = ({ children }) => {
  const { status, modelStatus, retryModelEnvironment } = useCloudAuth();
  const { t } = useTranslation();

  // Do not paint the authenticated shell while the cloud session is still
  // being checked. The old ordering allowed the conversation route to mount
  // model selectors during the login/restart gap, before sync and the task
  // resolver had produced an authoritative catalog.
  if (status === 'checking') {
    return (
      <div className='flex h-full min-h-100vh flex-col items-center justify-center gap-12px bg-[var(--color-bg-1)]'>
        <div className='text-16px font-medium text-t-primary'>{t('common.cloudModelEnvironment.restoringTitle')}</div>
        <div className='text-13px text-t-secondary'>{t('common.cloudModelEnvironment.restoringDescription')}</div>
      </div>
    );
  }

  if (status !== 'authenticated' || modelStatus === 'ready' || modelStatus === 'idle') {
    return <>{children}</>;
  }

  if (modelStatus === 'restoring') {
    return (
      <div className='flex h-full min-h-100vh flex-col items-center justify-center gap-12px bg-[var(--color-bg-1)]'>
        <div className='text-16px font-medium text-t-primary'>{t('common.cloudModelEnvironment.restoringTitle')}</div>
        <div className='text-13px text-t-secondary'>{t('common.cloudModelEnvironment.restoringDescription')}</div>
      </div>
    );
  }

  if (modelStatus === 'failed') {
    return (
      <div className='flex h-full min-h-100vh flex-col items-center justify-center gap-12px bg-[var(--color-bg-1)] px-24px'>
        <Alert
          type='error'
          title={t('common.cloudModelEnvironment.failedTitle')}
          content={t('common.cloudModelEnvironment.failedDescription')}
          className='max-w-520px'
        />
        <Button type='primary' onClick={() => void retryModelEnvironment()}>
          {t('common.cloudModelEnvironment.retry')}
        </Button>
      </div>
    );
  }

  return (
    <div className='flex h-full min-h-100vh flex-col bg-[var(--color-bg-1)]'>
      <Alert
        type='warning'
        title={t('common.cloudModelEnvironment.degradedTitle')}
        content={t('common.cloudModelEnvironment.degradedDescription')}
        action={
          <Button type='text' size='small' onClick={() => void retryModelEnvironment()}>
            {t('common.cloudModelEnvironment.retry')}
          </Button>
        }
      />
      <div className='min-h-0 flex-1'>{children}</div>
    </div>
  );
};

const Main = () => {
  const { ready, status } = useAuth();
  const { ready: cloudReady, refresh: refreshCloudAuth } = useCloudAuth();
  const [configReady, setConfigReady] = useState(false);
  const [configError, setConfigError] = useState<Error | null>(null);

  useEffect(() => {
    // Browser sessions must pass the auth probe before any protected startup
    // request runs. In particular, `/api/system/info` returns 403 for an
    // expired session; starting it while unauthenticated would turn the normal
    // login transition into an application-level render failure.
    //
    // Cloud whoami is intentionally not a config barrier: desktop cloud-login
    // is enforced by ProtectedLayout, and waiting here serializes an extra
    // round-trip before the first authenticated paint.
    if (!ready || status !== 'authenticated') {
      setConfigReady(false);
      setConfigError(null);
      return;
    }

    let active = true;
    setConfigReady(false);
    setConfigError(null);
    // Prefetch `/api/agents` in the background so Guid selectors warm the SWR
    // cache without blocking the first paint on PATH scanning / agent list IO.
    void fetchDetectedAgents()
      .then((agents) => swrMutate(DETECTED_AGENTS_SWR_KEY, agents, { revalidate: false }))
      .catch((err) => {
        console.error('Failed to prefetch agents:', err);
      });
    void Promise.all([
      application.systemInfo.invoke().catch((err) => {
        console.error('Failed to initialize browser storage generation:', err);
        throw err;
      }),
      configService.initialize().catch((err) => {
        console.error('Failed to initialize config:', err);
      }),
    ])
      .then(async ([info]) => {
        let generationChanged = false;
        try {
          generationChanged = getBrowserStorageGeneration() !== info.storageGeneration;
        } catch {
          // The first authenticated bootstrap has no prior generation.
        }
        setBrowserStorageGeneration(info.storageGeneration);
        if (generationChanged) {
          await clearAvailableModelsCache();
          await configService.reload();
          await refreshCloudAuth();
        }
        if (active) setConfigReady(true);
      })
      .catch((error: unknown) => {
        // httpBridge already cleared the expired browser session and notified
        // AuthProvider. Let the auth state render `/login`; never latch this
        // expected transition into the root error boundary.
        if (!active || isHandledAuthExpiredHttpError(error)) return;
        setConfigError(error instanceof Error ? error : new Error(String(error)));
      });

    return () => {
      active = false;
    };
  }, [ready, status, refreshCloudAuth]);

  useEffect(() => {
    if (!configReady || !shouldScheduleAgentRefreshForHash(window.location.hash)) return;

    type IdleWindow = Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };

    let active = true;
    let previousHash = window.location.hash;
    let cancelScheduledRefresh = () => {};
    const scheduleRefresh = () => {
      cancelScheduledRefresh();
      if (!shouldScheduleAgentRefreshForHash(window.location.hash)) return;
      const idleWindow = window as IdleWindow;
      const refresh = () => {
        cancelScheduledRefresh = () => {};
        if (active && document.visibilityState === 'visible') void refreshDetectedAgentsIfStale();
      };

      if (typeof idleWindow.requestIdleCallback === 'function') {
        const idleId = idleWindow.requestIdleCallback(refresh, { timeout: 5000 });
        cancelScheduledRefresh = () => {
          idleWindow.cancelIdleCallback?.(idleId);
        };
        return;
      }

      const timeoutId = window.setTimeout(refresh, 1000);
      cancelScheduledRefresh = () => window.clearTimeout(timeoutId);
    };

    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') scheduleRefresh();
    };
    const onHashChange = () => {
      const nextHash = window.location.hash;
      if (!shouldScheduleAgentRefreshForHash(nextHash)) {
        cancelScheduledRefresh();
      } else if (shouldScheduleAgentRefreshAfterHashChange(previousHash, nextHash)) {
        scheduleRefresh();
      }
      previousHash = nextHash;
    };
    scheduleRefresh();
    document.addEventListener('visibilitychange', onVisibilityChange);
    window.addEventListener('focus', scheduleRefresh);
    window.addEventListener('hashchange', onHashChange);

    return () => {
      active = false;
      cancelScheduledRefresh();
      document.removeEventListener('visibilitychange', onVisibilityChange);
      window.removeEventListener('focus', scheduleRefresh);
      window.removeEventListener('hashchange', onHashChange);
    };
  }, [configReady]);

  useEffect(() => {
    if (!ready || status !== 'authenticated') return;
    // Retention / cron repair can wait until cloud status is known on desktop,
    // but must not block first paint.
    if (!cloudReady) return;
    void repairAllCronJobTimeZonesOnce();
    maybeTrackRetention();
  }, [ready, cloudReady, status]);

  const router = (
    <Router
      layout={
        <ConversationHistoryProvider>
          <Layout sider={<Sider />} />
        </ConversationHistoryProvider>
      }
    />
  );

  if (!ready) {
    return <AppLoader />;
  }

  // The login route is intentionally independent from authenticated startup
  // data. This also makes an in-flight session expiry recover immediately.
  if (status !== 'authenticated') {
    return router;
  }

  if (configError) {
    throw configError;
  }

  if (!configReady) {
    return <AppLoader />;
  }

  return <CloudModelEnvironmentGate>{router}</CloudModelEnvironmentGate>;
};

const App = HOC.Wrapper(Config)(Main);

void registerPwa();

const root = createRoot(document.getElementById('root')!);
root.render(
  <RouteErrorBoundary scope='application'>
    <AppProviders>
      <App />
    </AppProviders>
  </RouteErrorBoundary>
);
