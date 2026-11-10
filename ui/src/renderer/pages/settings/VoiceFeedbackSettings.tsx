import React, { useEffect, useRef, useState } from 'react';
import { Button, Input, Select, Slider, Switch } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useTranslation } from 'react-i18next';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';
import { getVoiceFeedbackSettings, saveVoiceFeedback, getVoiceInputDevices, type VoiceFeedbackConfig, type VoiceFeedbackSettings, type AudioDeviceInfo } from '@common/adapter/voiceCompanion';
import { BRAND_WAKE_WORD } from '@/renderer/config/brand';

const UnsavedBadge: React.FC<{ text: string }> = ({ text }) => (
  <span className='inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-10px font-medium bg-warning-1 text-warning-6 border border-solid border-warning-3 animate-pulse motion-reduce:animate-none'>
    <span className='w-1.5 h-1.5 rounded-full bg-warning-6' />
    {text}
  </span>
);

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
          wake_words: value.requested.wake_words ?? [BRAND_WAKE_WORD],
          // Backend owns the authoritative product default; this is a display fallback only.
          tts_speed: value.requested.tts_speed ?? 1.1,
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
      const payload = {
        ...draft,
        input_device: draft.input_device && draft.input_device !== 'default' ? draft.input_device : 'default',
      };
      const res = await saveVoiceFeedback(payload);
      setSettings(res);
      setDraft(res.requested);
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
  const wakeEnabledDirty = Boolean(draft && settings && draft.wake_enabled !== settings.requested.wake_enabled);
  const wakeWordsDirty = Boolean(draft && settings && JSON.stringify(draft.wake_words ?? []) !== JSON.stringify(settings.requested.wake_words ?? []));
  const deviceDirty = Boolean(draft && settings && (draft.input_device || '') !== (settings.requested.input_device || ''));
  const volumeDirty = Boolean(draft && settings && draft.volume !== settings.requested.volume);
  const speedDirty = Boolean(draft && settings && draft.tts_speed !== settings.requested.tts_speed);
  const timeoutDirty = Boolean(draft && settings && draft.idle_timeout_secs !== settings.requested.idle_timeout_secs);

  const dirty = wakeEnabledDirty || wakeWordsDirty || deviceDirty || volumeDirty || speedDirty || timeoutDirty;
  const applied = Boolean(effective && settings && !dirty);

  return (
    <SettingsSection title={t('voice.feedback.voicePreferencesTitle')} description={t('voice.feedback.voicePreferencesDesc')}>
      <SettingsList>
        <div className={`p-4 rounded-8px border border-solid transition-all ${
          wakeEnabledDirty || wakeWordsDirty
            ? 'border-warning-5 bg-warning-1 shadow-[0_0_0_1px_rgba(var(--warning-6),0.2)]'
            : 'border-[var(--border-base)] bg-fill-1'
        }`}>
          <div className='flex items-center justify-between gap-4'>
            <div className='flex flex-col gap-1 min-w-0 flex-1'>
              <div className='text-14px font-medium text-t-primary flex items-center gap-2'>
                <span>{t('voice.feedback.wakeEnabled')}</span>
                {(wakeEnabledDirty || wakeWordsDirty) && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
              </div>
              <div className='text-12px text-t-secondary leading-relaxed'>
                {t('voice.feedback.wakeEnabledHint')}
              </div>
            </div>
            <div className='flex items-center gap-3 shrink-0'>
              <Input
                aria-label={t('voice.feedback.wakeWords')}
                placeholder={t('voice.feedback.wakeWordsPlaceholder')}
                maxLength={12}
                value={
                  !draft?.wake_words?.length || draft.wake_words[0].trim().toLowerCase() === BRAND_WAKE_WORD.toLowerCase()
                    ? ''
                    : draft.wake_words[0]
                }
                allowClear
                disabled={!draft || !(draft.wake_enabled ?? true) || busy}
                className={`w-160px transition-all ${wakeWordsDirty ? '!border-warning-5 shadow-[0_0_0_2px_rgba(var(--warning-5),0.4)] !bg-warning-1' : ''}`}
                onChange={(val) => {
                  const word = val.trim();
                  setDraft((prev) => prev && { ...prev, wake_words: word ? [word] : [] });
                }}
              />
              <Switch
                aria-label={t('voice.feedback.wakeEnabled')}
                checked={draft?.wake_enabled ?? true}
                disabled={!draft || busy}
                onChange={(val) => setDraft((prev) => prev && { ...prev, wake_enabled: val })}
              />
            </div>
          </div>
        </div>

        <SettingsRow
          label={
            <span className='flex items-center gap-2'>
              <span>{t('voice.feedback.inputDevice')}</span>
              {deviceDirty && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
            </span>
          }
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
              className={`min-w-240px transition-all ${deviceDirty ? '!border-warning-5 shadow-[0_0_0_2px_rgba(var(--warning-5),0.4)]' : ''}`}
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
          label={
            <span className='flex items-center gap-2'>
              <span>{t('voice.feedback.volume')}</span>
              {volumeDirty && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
            </span>
          }
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
          label={
            <span className='flex items-center gap-2'>
              <span>{t('voice.feedback.ttsSpeed')}</span>
              {speedDirty && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
            </span>
          }
          description={t('voice.feedback.ttsSpeedHint')}
          control={
            <div className='flex items-center gap-3'>
              <Slider
                aria-label={t('voice.feedback.ttsSpeed')}
                className='flex-1'
                min={0.5}
                max={2.0}
                step={0.1}
                value={draft?.tts_speed ?? 1.1}
                disabled={!draft || busy}
                onChange={(speed) => setDraft((value) => value && { ...value, tts_speed: Number(speed) })}
              />
              <span className='text-t-secondary tabular-nums'>{draft?.tts_speed ? `${draft.tts_speed.toFixed(1)}x` : '1.1x'}</span>
            </div>
          }
        />
        <SettingsRow
          label={
            <span className='flex items-center gap-2'>
              <span>{t('voice.feedback.idleTimeout')}</span>
              {timeoutDirty && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
            </span>
          }
          description={t('voice.feedback.idleTimeoutHint')}
          control={
            <Select
              aria-label={t('voice.feedback.idleTimeout')}
              value={draft?.idle_timeout_secs ?? 30}
              disabled={!draft || busy}
              onChange={(secs: number) => setDraft((value) => value && { ...value, idle_timeout_secs: Number(secs) })}
              className={`min-w-160px transition-all ${timeoutDirty ? '!border-warning-5 shadow-[0_0_0_2px_rgba(var(--warning-5),0.4)]' : ''}`}
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
                    ? '!bg-primary-6 !border-primary-6 text-white font-bold shadow-lg shadow-[0_0_0_3px_rgba(var(--primary-3),0.5)] hover:!bg-primary-7 animate-pulse motion-reduce:animate-none'
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
