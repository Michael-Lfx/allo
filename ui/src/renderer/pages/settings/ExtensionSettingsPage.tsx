

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { extensions as extensionsIpc, type IExtensionSettingsTab } from '@/common/adapter/ipcBridge';
import { useExtI18n } from '@/renderer/hooks/system/useExtI18n';
import { useExtensionSettingsTabs } from '@/renderer/hooks/system/useExtensionSettingsTabs';
import WebviewHost from '@/renderer/components/media/WebviewHost';
import { resolveExtensionAssetUrl } from '@/renderer/utils/platform';
import { Button } from '@arco-design/web-react';
import {
  SettingsEmptyState,
  SettingsPageHeader,
  SettingsStatus,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const isExternalSettingsUrl = (url?: string): boolean => /^https?:\/\//i.test(url || '');

/**
 * Route-based page for rendering extension-contributed settings tabs.
 * Loaded at `/settings/ext/:tabId` in the router.
 */
const ExtensionSettingsPage: React.FC = () => {
  const { tabId } = useParams<{ tabId: string }>();
  const { i18n, t } = useTranslation();
  const { resolveExtTabName } = useExtI18n();
  const { tabs: extensionTabs, status, refresh } = useExtensionSettingsTabs();
  const [loading, setLoading] = useState(true);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const pageState = useMemo<'loading' | 'empty' | 'not-found' | 'failed' | 'ready'>(() => {
    if (!tabId) {
      return 'not-found';
    }
    if (status === 'loading') {
      return 'loading';
    }
    if (status === 'error') {
      return 'failed';
    }
    if (extensionTabs.length === 0) return 'empty';
    return extensionTabs.some((item) => item.id === tabId) ? 'ready' : 'not-found';
  }, [extensionTabs, status, tabId]);

  const tab = useMemo(
    () => extensionTabs.find((item) => item.id === tabId) ?? null,
    [extensionTabs, tabId]
  );

  const resolvedUrl = resolveExtensionAssetUrl(tab?.url) ?? tab?.url;
  const isExternalTab = isExternalSettingsUrl(resolvedUrl);

  useEffect(() => {
    setLoading(true);
  }, [tab?.id, resolvedUrl]);

  const postLocaleInit = useCallback(async () => {
    if (!tab || isExternalTab) return;

    const frameWindow = iframeRef.current?.contentWindow;
    if (!frameWindow) return;

    try {
      const mergedI18n = await extensionsIpc.getExtI18nForLocale.invoke({ locale: i18n.language });
      const translations = (mergedI18n?.[tab.extension_name] as Record<string, unknown> | undefined) ?? {};

      frameWindow.postMessage(
        {
          type: 'nomi:init',
          locale: i18n.language,
          extensionName: tab.extension_name,
          translations,
        },
        '*'
      );
    } catch (err) {
      console.error('[ExtensionSettingsPage] Failed to post locale init:', err);
    }
  }, [i18n.language, isExternalTab, tab]);

  useEffect(() => {
    if (!tab || isExternalTab) return;

    const onMessage = async (event: MessageEvent) => {
      const frameWindow = iframeRef.current?.contentWindow;
      if (!frameWindow || event.source !== frameWindow) return;

      const data = event.data as { type?: string; reqId?: string } | undefined;
      if (!data) return;

      if (data.type === 'nomi:get-locale') {
        void postLocaleInit();
        return;
      }

      if (data.type !== 'star-office:request-snapshot') return;

      try {
        const snapshot = await extensionsIpc.getAgentActivitySnapshot.invoke();
        frameWindow.postMessage(
          {
            type: 'star-office:activity-snapshot',
            reqId: data.reqId,
            snapshot,
          },
          '*'
        );
      } catch (err) {
        console.error('[ExtensionSettingsPage] Failed to get activity snapshot:', err);
      }
    };

    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [isExternalTab, postLocaleInit, tab]);

  useEffect(() => {
    if (!loading) {
      void postLocaleInit();
    }
  }, [loading, postLocaleInit]);

  return (
    <SettingsPageWrapper layout='hub' loading={pageState === 'loading' && extensionTabs.length === 0}>
      <div className='space-y-24px'>
        <SettingsPageHeader
          title={tab ? resolveExtTabName(tab) : t('settings.extensionSettingsTitle')}
        />
        <div className='relative w-full min-h-400px'>
        {pageState === 'loading' && (
          <div className='absolute inset-0 flex items-center justify-center text-t-secondary text-14px'>
            <SettingsStatus>{t('settings.extensionSettingsLoading')}</SettingsStatus>
          </div>
        )}
        {pageState !== 'loading' && pageState !== 'ready' && (
          <SettingsEmptyState
            title={
              pageState === 'empty'
                ? t('settings.extensionSettingsEmpty')
                : pageState === 'failed'
                  ? t('settings.extensionSettingsFailed')
                  : t('settings.extensionSettingsNotFound')
            }
            action={pageState === 'failed' ? (
              <Button type='secondary' size='small' onClick={() => void refresh()}>
                {t('settings.extensionSettingsRetry')}
              </Button>
            ) : undefined}
          />
        )}
        {tab &&
          (isExternalTab ? (
            <WebviewHost
              key={tab.id}
              url={resolvedUrl || ''}
              id={tab.id}
              partition={`persist:ext-settings-${tab.id}`}
              style={{
                minHeight: '400px',
                height: 'calc(100vh - 200px)',
              }}
            />
          ) : (
            <>
              {loading && (
                <div className='absolute inset-0 flex items-center justify-center text-t-secondary text-14px'>
                  <SettingsStatus>{t('settings.extensionSettingsLoading')}</SettingsStatus>
                </div>
              )}
              <iframe
                ref={iframeRef}
                key={tab.id}
                src={resolvedUrl}
                onLoad={() => setLoading(false)}
                sandbox='allow-scripts allow-same-origin'
                className='w-full border-none'
                style={{
                  minHeight: '400px',
                  height: 'calc(100vh - 200px)',
                  opacity: loading ? 0 : 1,
                  transition: 'opacity 150ms ease-in',
                }}
                title={`Extension settings: ${resolveExtTabName(tab)}`}
              />
            </>
          ))}
        </div>
      </div>
    </SettingsPageWrapper>
  );
};

export default ExtensionSettingsPage;
