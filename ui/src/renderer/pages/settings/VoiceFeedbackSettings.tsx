import React, { useEffect, useRef, useState } from 'react';
import { Button, Select, Slider, Switch } from '@arco-design/web-react';
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
      if (alive && revision === loadRevision.current) { setSettings(value); setDraft((current) => current ?? value.requested); setError(false); }
    }).catch(() => { if (alive && revision === loadRevision.current) setError(true); });
    return () => { alive = false; };
  }, [connected, refresh]);
  const save = async () => {
    if (!draft) return;
    ++loadRevision.current;
    setBusy(true);
    try { setSettings(await saveVoiceFeedback(draft)); setError(false); }
    catch { setError(true); }
    finally { setBusy(false); }
  };
  const preview = async () => {
    setBusy(true); setPreviewError(null);
    try { await previewVoiceFeedback(kind); }
    catch (cause) { setPreviewError(cause instanceof Error ? cause.message : 'feedback_request_failed'); }
    finally { setBusy(false); }
  };
  const effective = settings?.effective;
  const dirty = draft && settings && (draft.mode !== settings.requested.mode || draft.volume !== settings.requested.volume || draft.speak_replies !== settings.requested.speak_replies);
  const applied = effective && settings && effective.mode === settings.requested.mode && effective.volume === settings.requested.volume && effective.speak_replies === settings.requested.speak_replies;
  const kinds: VoicePreviewKind[] = ['wake', 'recognized', 'processing', 'follow_up', 'sleep', 'error'];
  return <SettingsSection title={t('voice.feedback.title')} description={t('voice.feedback.description')}>
    <SettingsList>
      <SettingsRow label={t('voice.feedback.mode')} description={t('voice.feedback.modeHint')} control={<Select aria-label={t('voice.feedback.mode')} value={draft?.mode} disabled={!draft || busy} onChange={(mode: VoiceFeedbackConfig['mode']) => setDraft((value) => value && {...value, mode})}>
        <Select.Option value='light'>{t('voice.feedback.light')}</Select.Option>
        <Select.Option value='essential'>{t('voice.feedback.essential')}</Select.Option>
        <Select.Option value='off'>{t('voice.feedback.off')}</Select.Option>
      </Select>} />
      <SettingsRow label={t('voice.feedback.volume')} description={t('voice.feedback.volumeHint')} control={<div className='flex items-center gap-3'><Slider aria-label={t('voice.feedback.volume')} className='flex-1' min={0} max={100} value={draft?.volume} disabled={!draft || busy} onChange={(volume) => setDraft((value) => value && {...value, volume: Number(volume)})} /><span className='text-t-secondary tabular-nums'>{draft ? `${draft.volume}%` : '—'}</span></div>} />
      <SettingsRow label={t('voice.feedback.speakReplies')} description={t('voice.feedback.speakRepliesHint')} controlLayout='compact' control={<Switch aria-label={t('voice.feedback.speakReplies')} checked={draft?.speak_replies ?? false} disabled={!draft || busy} onChange={(speak_replies) => setDraft((value) => value && {...value, speak_replies})} />} />
      <SettingsRow label={t('voice.feedback.preview')} description={t('voice.feedback.previewHint')} controlLayout='compound' control={<div className='flex flex-wrap items-center gap-3'>
        <Select aria-label={t('voice.feedback.preview')} value={kind} disabled={busy} onChange={setKind} className='min-w-160px flex-1'>{kinds.map((value) => <Select.Option key={value} value={value}>{t(`voice.feedback.cue.${value}`)}</Select.Option>)}</Select>
        <Button disabled={!connected || !effective || effective.volume === 0 || effective.mode === 'off' || busy} onClick={() => void preview()}>{t('voice.feedback.previewButton')}</Button>
      </div>} />
      <SettingsRow label={t('voice.feedback.status')} description={<span role={error ? 'alert' : 'status'}>{error ? t('voice.feedback.failed') : !settings ? t('voice.feedback.loading') : dirty ? t('voice.feedback.unsaved') : applied ? t('voice.feedback.applied') : t('voice.feedback.pending')}</span>} controlLayout='actions' control={<div className='flex flex-wrap gap-3'>
        <Button disabled={busy} onClick={() => setRefresh((value) => value + 1)}>{t('voice.service.retrySync')}</Button>
        <Button type='primary' loading={busy} disabled={!draft} onClick={() => void save()}>{t('voice.feedback.save')}</Button>
      </div>} />
    </SettingsList>
    {previewError && <p role='alert' className='text-t-secondary'>{previewError === 'voice_busy' ? t('voice.feedback.previewBusy') : previewError === 'feedback_muted' ? t('voice.feedback.previewMuted') : t('voice.feedback.previewFailed')}</p>}
  </SettingsSection>;
}
