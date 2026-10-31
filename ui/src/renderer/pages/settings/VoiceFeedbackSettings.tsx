import React, { useEffect, useRef, useState } from 'react';
import { Button, Input, Message, Radio, Select, Slider, Switch } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';
import { getVoiceFeedbackSettings, saveVoiceFeedback, previewVoiceFeedback, type VoiceFeedbackConfig, type VoiceFeedbackSettings, type VoicePreviewKind } from '@common/adapter/voiceCompanion';

export default function VoiceFeedbackSettingsSection({ connected }: { connected: boolean }) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<VoiceFeedbackSettings | null>(null);
  const [draft, setDraft] = useState<VoiceFeedbackConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [kind, setKind] = useState<VoicePreviewKind>('wake');
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
          wake_words: value.requested.wake_words ?? ['Hey Flowy', '你好 Flowy'],
          wake_reply: value.requested.wake_reply ?? '我在',
          interject_keyword: value.requested.interject_keyword ?? 'Flowy',
          dismiss_phrases: value.requested.dismiss_phrases ?? ['退下吧', '结束通话', '休息吧'],
          dismiss_reply: value.requested.dismiss_reply ?? '好的，再见',
          tts_backend: value.requested.tts_backend ?? 'local',
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

  const preview = async () => {
    setBusy(true); setPreviewError(null);
    try { await previewVoiceFeedback(kind); }
    catch (cause) { setPreviewError(cause instanceof Error ? cause.message : 'feedback_request_failed'); }
    finally { setBusy(false); }
  };

  const effective = settings?.effective;
  const dirty = Boolean(draft && settings && (
    draft.mode !== settings.requested.mode ||
    draft.volume !== settings.requested.volume ||
    draft.speak_replies !== settings.requested.speak_replies ||
    JSON.stringify(draft.wake_words ?? []) !== JSON.stringify(settings.requested.wake_words ?? []) ||
    draft.wake_reply !== settings.requested.wake_reply ||
    draft.interject_keyword !== settings.requested.interject_keyword ||
    JSON.stringify(draft.dismiss_phrases ?? []) !== JSON.stringify(settings.requested.dismiss_phrases ?? []) ||
    draft.dismiss_reply !== settings.requested.dismiss_reply ||
    draft.tts_backend !== settings.requested.tts_backend ||
    draft.tts_speed !== settings.requested.tts_speed ||
    draft.idle_timeout_secs !== settings.requested.idle_timeout_secs
  ));
  const applied = Boolean(effective && settings && !dirty);
  const kinds: VoicePreviewKind[] = ['wake', 'recognized', 'processing', 'follow_up', 'sleep', 'error'];

  return (
    <>
      {/* 卡片 1: 语音唤醒 (可编辑) */}
      <SettingsSection title={t('voice.feedback.wakeWords')} description={t('voice.feedback.wakeWordsHint')}>
        <SettingsList>
          <SettingsRow
            label={t('voice.feedback.wakeWords')}
            description={t('voice.feedback.wakeWordsHint')}
            controlLayout='field'
            control={
              <Input
                aria-label={t('voice.feedback.wakeWords')}
                placeholder={t('voice.feedback.wakeWordsPlaceholder')}
                value={draft?.wake_words?.[0] ?? 'Hey Flowy'}
                disabled={!draft || busy}
                onChange={(val) => {
                  const word = val.trim();
                  setDraft((prev) => prev && { ...prev, wake_words: word ? [word] : ['Hey Flowy'] });
                }}
              />
            }
          />
        </SettingsList>
      </SettingsSection>

      {/* 卡片 2: 语音合成与播报 (可编辑) */}
      <SettingsSection title={t('voice.feedback.title')} description={t('voice.feedback.description')}>
        <SettingsList>
          <SettingsRow
            label={t('voice.feedback.ttsBackend')}
            description={t('voice.feedback.ttsBackendHint')}
            controlLayout='field'
            control={
              <Radio.Group
                aria-label={t('voice.feedback.ttsBackend')}
                type='button'
                value={draft?.tts_backend ?? 'local'}
                disabled={!draft || busy}
                onChange={(val) => setDraft((prev) => prev && { ...prev, tts_backend: val })}
              >
                <Radio value='local'>{t('voice.feedback.ttsBackendLocal')}</Radio>
                <Radio value='edge'>{t('voice.feedback.ttsBackendEdge')}</Radio>
              </Radio.Group>
            }
          />
          <SettingsRow
            label={t('voice.feedback.speakReplies')}
            description={t('voice.feedback.speakRepliesHint')}
            controlLayout='compact'
            control={
              <Switch
                aria-label={t('voice.feedback.speakReplies')}
                checked={draft?.speak_replies ?? true}
                disabled={!draft || busy}
                onChange={(speak_replies) => setDraft((value) => value && { ...value, speak_replies })}
              />
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
            label={t('voice.feedback.mode')}
            description={t('voice.feedback.modeHint')}
            control={
              <Select
                aria-label={t('voice.feedback.mode')}
                value={draft?.mode ?? 'light'}
                disabled={!draft || busy}
                onChange={(mode: VoiceFeedbackConfig['mode']) => setDraft((value) => value && { ...value, mode })}
              >
                <Select.Option value='light'>{t('voice.feedback.light')}</Select.Option>
                <Select.Option value='essential'>{t('voice.feedback.essential')}</Select.Option>
                <Select.Option value='off'>{t('voice.feedback.off')}</Select.Option>
              </Select>
            }
          />
          <SettingsRow
            label={t('voice.feedback.preview')}
            description={t('voice.feedback.previewHint')}
            controlLayout='compound'
            control={
              <div className='flex flex-wrap items-center gap-3'>
                <Select
                  aria-label={t('voice.feedback.preview')}
                  value={kind}
                  disabled={busy}
                  onChange={setKind}
                  className='min-w-160px flex-1'
                >
                  {kinds.map((value) => (
                    <Select.Option key={value} value={value}>{t(`voice.feedback.cue.${value}`)}</Select.Option>
                  ))}
                </Select>
                <Button
                  disabled={!connected || !effective || effective.volume === 0 || effective.mode === 'off' || busy}
                  onClick={() => void preview()}
                >
                  {t('voice.feedback.previewButton')}
                </Button>
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
                {error
                  ? t('voice.feedback.failed')
                  : !settings
                    ? t('voice.feedback.loading')
                    : dirty
                      ? t('voice.feedback.unsaved')
                      : applied
                        ? t('voice.feedback.applied')
                        : t('voice.feedback.pending')}
              </span>
            }
            controlLayout='actions'
            control={
              <div className='flex flex-wrap gap-3'>
                <Button disabled={busy} onClick={() => setRefresh((value) => value + 1)}>
                  {t('voice.service.retrySync')}
                </Button>
                <Button type='primary' loading={busy} disabled={!draft || !dirty} onClick={() => void save()}>
                  {t('voice.feedback.savePreferences')}
                </Button>
              </div>
            }
          />
        </SettingsList>
        {previewError && (
          <p role='alert' className='text-t-secondary'>
            {previewError === 'voice_busy'
              ? t('voice.feedback.previewBusy')
              : previewError === 'feedback_muted'
                ? t('voice.feedback.previewMuted')
                : t('voice.feedback.previewFailed')}
          </p>
        )}
      </SettingsSection>
    </>
  );
}
