import React, { useEffect, useRef, useState } from 'react';
import { Button, Input, Message, Select, Slider, Switch } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';
import { getVoiceFeedbackSettings, saveVoiceFeedback, getVoiceInputDevices, type VoiceFeedbackConfig, type VoiceFeedbackSettings, type AudioDeviceInfo } from '@common/adapter/voiceCompanion';

const UnsavedBadge: React.FC<{ text: string }> = ({ text }) => (
  <span className='inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-10px font-medium bg-warning-1 text-warning-6 border border-solid border-warning-3 animate-pulse motion-reduce:animate-none'>
    <span className='w-1.5 h-1.5 rounded-full bg-warning-6' />
    {text}
  </span>
);

const isKwsDirectSupported = (word?: string): boolean => {
  if (!word || !word.trim()) return true;
  const clean = word.trim().toLowerCase();
  if (clean === 'hey flowy' || clean === 'flowy') return true;
  if (/^[a-zA-Z\s,.'!?-]+$/.test(clean)) return true;
  const supportedChars = new Set([
    '你','好','小','白','福','星','芙','弗','伏','服','符','浮',
    '莉','力','利','立','佛','罗','洛','嗨','嘿','黑','哈','喽',
    '楼','宝','贝','同','学','助','手','智','能','管','家','飞',
    '爱','度','达','大','米','奇','华','为','维','灵','猫','毛',
    '天','甜','精','金','美','妹','特','斯','四','丁','旺','王',
    '财','豆','卡','波','伯','比','布','多','朵','可','乐','欢',
    '迎','银','电','脑','呼','虎','叫','唤','贾','安','娜','莫',
    '妮','希','瑞','龙','兔','狗','阿','文','森','木','杰','克',
    '西','球','咪','香','芋','片','果','茶','花','草','糖','雪',
    '冰','鱼','鸟','月','日'
  ]);
  const chars = clean.replace(/[\s，。！？,.\-_!?:：；;、—…]+/g, '').split('');
  return chars.length > 0 && chars.every((c) => supportedChars.has(c));
};

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
        <div className={`flex flex-col gap-3 p-4 rounded-8px border border-solid transition-all ${
          wakeEnabledDirty || wakeWordsDirty
            ? 'border-warning-5/60 bg-warning-1/5 shadow-[0_0_0_1px_rgba(var(--warning-6),0.2)]'
            : 'border-[var(--border-base)] bg-fill-1'
        }`}>
          <div className='flex items-center justify-between'>
            <div className='flex flex-col gap-0.5'>
              <div className='text-14px font-medium text-t-primary flex items-center gap-2'>
                <span>{t('voice.feedback.wakeEnabled')}</span>
                {wakeEnabledDirty && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
              </div>
              <div className='text-12px text-t-secondary'>
                {t('voice.feedback.wakeEnabledHint')}
              </div>
            </div>
            <Switch
              aria-label={t('voice.feedback.wakeEnabled')}
              checked={draft?.wake_enabled ?? true}
              disabled={!draft || busy}
              onChange={(val) => setDraft((prev) => prev && { ...prev, wake_enabled: val })}
            />
          </div>

          {(draft?.wake_enabled ?? true) && (
            <div className='flex flex-col gap-2 pt-3 border-t border-t-solid border-[var(--border-base)]'>
              <div className='flex items-center justify-between'>
                <div className='text-13px font-medium text-t-primary flex items-center gap-2'>
                  <span>{t('voice.feedback.wakeWords')}</span>
                  {wakeWordsDirty && <UnsavedBadge text={t('voice.feedback.unsaved')} />}
                </div>
                <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-11px font-medium border border-solid ${
                  isKwsDirectSupported(draft?.wake_words?.[0])
                    ? 'text-success-6 border-success-3 bg-success-1'
                    : 'text-primary-6 border-primary-3 bg-primary-1'
                }`}>
                  <span>{isKwsDirectSupported(draft?.wake_words?.[0]) ? '●' : '◈'}</span>
                  <span>{isKwsDirectSupported(draft?.wake_words?.[0]) ? t('voice.feedback.kwsDirectReady') : t('voice.feedback.cascadeFallbackReady')}</span>
                </span>
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
                className={`transition-all ${wakeWordsDirty ? '!border-warning-5 !ring-2 !ring-warning-5/30 !bg-warning-1/10' : ''}`}
                onChange={(val) => {
                  const word = val.trim();
                  setDraft((prev) => prev && { ...prev, wake_words: word ? [word] : [] });
                }}
              />
            </div>
          )}
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
              className={`min-w-240px transition-all ${deviceDirty ? '!border-warning-5 !ring-2 !ring-warning-5/30' : ''}`}
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
                value={draft?.tts_speed ?? 1.0}
                disabled={!draft || busy}
                onChange={(speed) => setDraft((value) => value && { ...value, tts_speed: Number(speed) })}
              />
              <span className='text-t-secondary tabular-nums'>{draft?.tts_speed ? `${draft.tts_speed.toFixed(1)}x` : '1.0x'}</span>
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
              className={`min-w-160px transition-all ${timeoutDirty ? '!border-warning-5 !ring-2 !ring-warning-5/30' : ''}`}
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
                    ? '!bg-primary-6 !border-primary-6 text-white font-bold shadow-lg ring-4 ring-primary-3/50 hover:!bg-primary-7 animate-pulse motion-reduce:animate-none'
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
