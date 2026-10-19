import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { hideVoiceWindow, stopVoiceSession, subscribeVoiceSession, wakeVoiceSession, type VoiceSnapshot } from '@common/adapter/voiceCompanion';
import { configService } from '@common/config/configService';
import { isTauriRuntime } from '@common/adapter/tauriRuntime';
import { injectCompanionCustomCss } from '@renderer/utils/theme/applyCustomCss';
import { injectWallpaperTokenSheet, restoreWallpaperTokensFromCache } from '@renderer/utils/theme/applyWallpaperTokens';
import { THEME_SYNC_EVENT, type ThemeSyncPayload } from '@renderer/utils/theme/themeBroadcast';
import VoiceAudioStatus from './VoiceAudioStatus';
import VoiceConversation from './VoiceConversation';
import VoiceTaskStatus from './VoiceTaskStatus';
import VoiceTaskDiagnostics from './VoiceTaskDiagnostics';
import VoiceErrorStatus from './VoiceErrorStatus';

export default function VoiceSessionPage() {
  const { t } = useTranslation();
  const [snapshot, setSnapshot] = useState<VoiceSnapshot | null>(null);
  const [failed, setFailed] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [retrying, setRetrying] = useState(false);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let receivedPreset = false;
    // ThemeProvider owns initial light/dark. This route has no Layout, so it
    // must also load its preset and receive the existing cross-window updates.
    void configService.whenReady().then(() => {
      if (disposed || receivedPreset) return;
      injectCompanionCustomCss((configService.get('customCss') as string) || '');
      restoreWallpaperTokensFromCache();
    }).catch(() => {});
    if (isTauriRuntime()) {
      void import('@tauri-apps/api/event').then(({ listen }) => listen<ThemeSyncPayload>(THEME_SYNC_EVENT, ({ payload }) => {
        if (disposed) return;
        if (payload.theme === 'light' || payload.theme === 'dark') {
          document.documentElement.setAttribute('data-theme', payload.theme);
          document.body.setAttribute('arco-theme', payload.theme);
        }
        if (payload.customCss !== undefined) {
          receivedPreset = true;
          injectCompanionCustomCss(payload.customCss);
        }
        if (payload.wallpaperTokens !== undefined) injectWallpaperTokenSheet(payload.wallpaperTokens, Boolean(payload.wallpaperEnabled));
      })).then((off) => { if (disposed) off(); else unlisten = off; }).catch(() => {});
    }
    return () => { disposed = true; unlisten?.(); };
  }, []);
  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void subscribeVoiceSession((next) => { if (!disposed) setSnapshot(next); }, undefined, (code) => { if (!disposed) setFailed(Boolean(code)); }).then((off) => {
      if (disposed) off(); else unsubscribe = off;
    }).catch(() => { if (!disposed) setFailed(true); });
    return () => { disposed = true; unsubscribe?.(); };
  }, []);
  const stop = async () => {
    if (!snapshot) return;
    setStopping(true);
    try { await stopVoiceSession(snapshot); await hideVoiceWindow(); }
    catch { setFailed(true); }
    finally { setStopping(false); }
  };
  return <main className='fixed inset-0 box-border flex flex-col bg-base text-t-primary p-5 gap-3 overflow-hidden'>
    <h1 className='text-lg font-semibold m-0'>{t('voice.session.title')}</h1>
    <div role='status' aria-live='polite'>{snapshot ? t(`voice.session.phase.${snapshot.phase}`) : t('voice.session.connecting')}</div>
    <div className='shrink-0'><VoiceAudioStatus snapshot={snapshot} includeError={false} compact /></div>
    <section className='flex-1 min-h-0 overflow-auto flex flex-col gap-3'>
      <VoiceTaskDiagnostics snapshot={snapshot} />
      <VoiceConversation snapshot={snapshot} />
      <VoiceTaskStatus task={snapshot?.task} errorCode={snapshot?.task_error_code} instanceId={snapshot?.instance_id} confirmation={snapshot?.confirmation} />
      {failed && <p role='alert'>{t('voice.session.unavailable')}</p>}
      <VoiceErrorStatus error={snapshot?.error ?? null} endedReason={snapshot?.ended_reason} />
    </section>
    {snapshot?.phase === 'idle' && <Button type='primary' long disabled={!snapshot.acoustic_ready || snapshot.connection !== 'connected'} loading={retrying} onClick={() => {
      setRetrying(true);
      void wakeVoiceSession().then(() => setFailed(false)).catch(() => setFailed(true)).finally(() => setRetrying(false));
    }}>{t('voice.session.retry')}</Button>}
    <Button type={snapshot?.phase === 'idle' ? 'secondary' : 'primary'} long disabled={!snapshot} loading={stopping} onClick={() => { void stop(); }}>{t('voice.session.stop')}</Button>
  </main>;
}
