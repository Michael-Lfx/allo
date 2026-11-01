import React, { useEffect, useRef, useState } from 'react';
import { Button, Input, Message, Select, Slider, Switch } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';
import { getVoiceFeedbackSettings, saveVoiceFeedback, getVoiceInputDevices, type VoiceFeedbackConfig, type VoiceFeedbackSettings, type AudioDeviceInfo } from '@common/adapter/voiceCompanion';

export default function VoiceFeedbackSettingsSection({ connected }: { connected: boolean }) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<VoiceFeedbackSettings | null>(null);
  const [draft, setDraft] = useState<VoiceFeedbackConfig | null>(null);
  const [devices, setDevices] = useState<AudioDeviceInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const loadRevision = useRef(0);

  useEffect(() => {
    let alive = true;
    const revision = ++loadRevision.current;
    void getVoiceFeedbackSettings().then((value) => {
      if (alive && revision === loadRevision.current) {
        setSettings(value);
        setDraft((current) => current ?? {
          ...value.requested,
          wake_enabled: value.requested.wake_enabled ?? true,
          wake_words: value.requested.wake_words ?? ['Hey Flowy'],
          tts_speed: value.requested.tts_speed ?? 1.0,
          idle_timeout_secs: value.requested.idle_timeout_secs ?? 30,
          input_device: value.requested.input_device,
        });
        setError(false);
      }
    }).catch(() => { if (alive && revision === loadRevision.current) setError(true); });

    void getVoiceInputDevices().then((devs) => {
      if (alive) setDevices(devs);
    });

    return () => { alive = false; };
  }, [connected]);

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
    draft.wake_enabled !== settings.requested.wake_enabled ||
    JSON.stringify(draft.wake_words ?? []) !== JSON.stringify(settings.requested.wake_words ?? []) ||
    draft.tts_speed !== settings.requested.tts_speed ||
    draft.idle_timeout_secs !== settings.requested.idle_timeout_secs ||
    (draft.input_device || '') !== (settings.requested.input_device || '')
  ));
  const applied = Boolean(effective && settings && !dirty);

  return (
    <SettingsSection title={t('voice.feedback.voicePreferencesTitle')} description={t('voice.feedback.voicePreferencesDesc')}>
      <SettingsList>
        <SettingsRow
          label={t('voice.feedback.wakeEnabled')}
          description={t('voice.feedback.wakeEnabledHint')}
          controlLayout='field'
          control={
            <div className='flex flex-col gap-3 w-full'>
              <div className='flex items-center justify-between'>
                <span className='text-12px text-t-secondary'>
                  {draft?.wake_enabled ?? true
                    ? t('voice.feedback.wakeWordsHint')
                    : t('voice.feedback.wakeDisabledNotice', { defaultValue: '语音唤醒已停用，空闲时不占用麦克风' })}
                </span>
                <Switch
                  aria-label={t('voice.feedback.wakeEnabled')}
                  checked={draft?.wake_enabled ?? true}
                  disabled={!draft || busy}
                  onChange={(val) => setDraft((prev) => prev && { ...prev, wake_enabled: val })}
                />
              </div>
              {(draft?.wake_enabled ?? true) && (
                <div className='flex flex-col gap-1.5 pt-1'>
                  <div className='text-12px font-medium text-t-primary'>
                    {t('voice.feedback.wakeWords')}
                  </div>
                  <Input
                    aria-label={t('voice.feedback.wakeWords')}
                    placeholder={t('voice.feedback.wakeWordsPlaceholder')}
                    value={
                      !draft?.wake_words?.length || draft.wake_words[0].trim().toLowerCase() === 'hey flowy'
                        ? ''
                        : draft.wake_words[0]
                    }
                    allowClear
                    disabled={!draft || busy}
                    onChange={(val) => {
                      const word = val.trim();
                      setDraft((prev) => prev && { ...prev, wake_words: word ? [word] : [] });
                    }}
                  />
                </div>
              )}
            </div>
          }
        />
        <SettingsRow
          label={t('voice.feedback.inputDevice')}
          description={t('voice.feedback.inputDeviceHint')}
          control={
            <Select
              aria-label={t('voice.feedback.inputDevice')}
              value={draft?.input_device || 'default'}
              disabled={!draft || busy}
              onChange={(val: string) => {
                setDraft((prev) => prev && {
                  ...prev,
                  input_device: val === 'default' ? undefined : val,
                });
              }}
              className='min-w-240px'
            >
              <Select.Option value='default'>{t('voice.feedback.inputDeviceDefault')}</Select.Option>
              {devices.map((d) => (
                <Select.Option key={d.id} value={d.name}>
                  {d.name} {d.is_default ? `(${t('voice.feedback.inputDeviceIsOsDefault')})` : ''}
                </Select.Option>
              ))}
            </Select>
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
                <span className='text-warning-6 font-semibold animate-pulse motion-reduce:animate-none flex items-center gap-1.5'>
                  <span className='inline-block w-2 h-2 rounded-full bg-warning-6 animate-ping motion-reduce:animate-none' />
                  {t('voice.feedback.unsaved')}
                </span>
              ) : applied ? (
                <span className='text-success-6 font-medium flex items-center gap-1.5'>
                  <span className='inline-block w-2 h-2 rounded-full bg-success-6' />
                  {t('voice.feedback.applied')}
                </span>
              ) : (
                t('voice.feedback.pending')
              )}
            </span>
          }
          controlLayout='actions'
          control={
            <div className='flex items-center gap-3'>
              <Button
                type='primary'
                loading={busy}
                disabled={!draft || !dirty}
                onClick={() => void save()}
                className={
                  dirty
                    ? 'ring-2 ring-[var(--flowy-attention)] !bg-[var(--flowy-attention)] !border-[var(--flowy-attention)] text-white font-bold shadow-lg animate-pulse motion-reduce:animate-none'
                    : ''
                }
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
