import React, { useEffect, useRef, useState } from 'react';
import { Button, Input, Message, Select, Slider } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';
import { getVoiceFeedbackSettings, saveVoiceFeedback, type VoiceFeedbackConfig, type VoiceFeedbackSettings } from '@common/adapter/voiceCompanion';

export default function VoiceFeedbackSettingsSection({ connected }: { connected: boolean }) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<VoiceFeedbackSettings | null>(null);
  const [draft, setDraft] = useState<VoiceFeedbackConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const loadRevision = useRef(0);

  useEffect(() => {
    let alive = true;
    const revision = ++loadRevision.current;
    void getVoiceFeedbackSettings().then((value) => {
      if (alive && revision === loadRevision.current) {
        setSettings(value);
        setDraft((current) => current ?? {
          ...value.requested,
          wake_words: value.requested.wake_words ?? ['Hey Flowy'],
          tts_speed: value.requested.tts_speed ?? 1.0,
          idle_timeout_secs: value.requested.idle_timeout_secs ?? 30,
        });
        setError(false);
      }
    }).catch(() => { if (alive && revision === loadRevision.current) setError(true); });
    return () => { alive = false; };
  }, [connected, refresh]);

  const save = async () => {
    if (!draft) return;
    ++loadRevision.current;
    setBusy(true);
    try {
      const res = await saveVoiceFeedback(draft);
      setSettings(res);
      setError(false);
      Message.success(t('voice.feedback.saveSuccess'));
    } catch {
      setError(true);
      Message.error(t('voice.feedback.failed'));
    } finally {
      setBusy(false);
    }
  };

  const effective = settings?.effective;
  const dirty = Boolean(draft && settings && (
    draft.volume !== settings.requested.volume ||
    JSON.stringify(draft.wake_words ?? []) !== JSON.stringify(settings.requested.wake_words ?? []) ||
    draft.tts_speed !== settings.requested.tts_speed ||
    draft.idle_timeout_secs !== settings.requested.idle_timeout_secs
  ));
  const applied = Boolean(effective && settings && !dirty);

  return (
    <SettingsSection title={t('voice.feedback.voicePreferencesTitle')} description={t('voice.feedback.voicePreferencesDesc')}>
      <SettingsList>
        <SettingsRow
          label={t('voice.feedback.wakeWords')}
          description={t('voice.feedback.wakeWordsHint')}
          controlLayout='field'
          control={
            <Input
              aria-label={t('voice.feedback.wakeWords')}
              placeholder={t('voice.feedback.wakeWordsPlaceholder')}
              value={draft?.wake_words?.[0] ?? ''}
              allowClear
              disabled={!draft || busy}
              onChange={(val) => {
                const word = val.trim();
                setDraft((prev) => prev && { ...prev, wake_words: word ? [word] : [] });
              }}
            />
          }
        />
        <SettingsRow
          label={t('voice.feedback.volume')}
          description={t('voice.feedback.volumeHint')}
          control={
            <div className='flex items-center gap-3'>
              <Slider
                aria-label={t('voice.feedback.volume')}
                className='flex-1'
                min={0}
                max={100}
                value={draft?.volume ?? 50}
                disabled={!draft || busy}
                onChange={(volume) => setDraft((value) => value && { ...value, volume: Number(volume) })}
              />
              <span className='text-t-secondary tabular-nums'>{draft ? `${draft.volume}%` : '—'}</span>
            </div>
          }
        />
        <SettingsRow
          label={t('voice.feedback.ttsSpeed')}
          description={t('voice.feedback.ttsSpeedHint')}
          control={
            <div className='flex items-center gap-3'>
              <Slider
                aria-label={t('voice.feedback.ttsSpeed')}
                className='flex-1'
                min={0.5}
                max={2.0}
                step={0.1}
                value={draft?.tts_speed ?? 1.0}
                disabled={!draft || busy}
                onChange={(speed) => setDraft((value) => value && { ...value, tts_speed: Number(speed) })}
              />
              <span className='text-t-secondary tabular-nums'>{draft?.tts_speed ? `${draft.tts_speed.toFixed(1)}x` : '1.0x'}</span>
            </div>
          }
        />
        <SettingsRow
          label={t('voice.feedback.idleTimeout')}
          description={t('voice.feedback.idleTimeoutHint')}
          control={
            <Select
              aria-label={t('voice.feedback.idleTimeout')}
              value={draft?.idle_timeout_secs ?? 30}
              disabled={!draft || busy}
              onChange={(secs: number) => setDraft((value) => value && { ...value, idle_timeout_secs: Number(secs) })}
            >
              <Select.Option value={15}>{t('voice.feedback.idle15')}</Select.Option>
              <Select.Option value={30}>{t('voice.feedback.idle30')}</Select.Option>
              <Select.Option value={60}>{t('voice.feedback.idle60')}</Select.Option>
              <Select.Option value={0}>{t('voice.feedback.idleNever')}</Select.Option>
            </Select>
          }
        />
        <SettingsRow
          label={t('voice.feedback.status')}
          description={
            <span role={error ? 'alert' : 'status'}>
              {error ? (
                <span className='text-danger-6'>{t('voice.feedback.failed')}</span>
              ) : !settings ? (
                t('voice.feedback.loading')
              ) : dirty ? (
                <span className='text-warning-6 font-medium animate-pulse'>● {t('voice.feedback.unsaved')}</span>
              ) : applied ? (
                <span className='text-success-6 font-medium'>● {t('voice.feedback.applied')}</span>
              ) : (
                t('voice.feedback.pending')
              )}
            </span>
          }
          controlLayout='actions'
          control={
            <div className='flex flex-wrap gap-3'>
              <Button disabled={busy} onClick={() => setRefresh((value) => value + 1)}>
                {t('voice.service.retrySync')}
              </Button>
              <Button
                type='primary'
                loading={busy}
                disabled={!draft || !dirty}
                onClick={() => void save()}
                className={dirty ? 'animate-pulse ring-2 ring-primary-5 font-semibold' : ''}
              >
                {t('voice.feedback.savePreferences')}
              </Button>
            </div>
          }
        />
      </SettingsList>
    </SettingsSection>
  );
}
