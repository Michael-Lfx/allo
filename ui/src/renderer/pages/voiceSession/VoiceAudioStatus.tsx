import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import VoiceErrorStatus from './VoiceErrorStatus';

/** Only daemon PCM telemetry is shown; this component never opens a microphone. */
export default function VoiceAudioStatus({ snapshot, includeError = true, compact = false }: { snapshot: VoiceSnapshot | null; includeError?: boolean; compact?: boolean }) {
  const { t } = useTranslation();
  const [now, setNow] = useState(Date.now);
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 500); return () => clearInterval(timer); }, []);
  const audio = snapshot?.audio;
  const recent = Boolean(audio?.last_frame_at_ms && now - audio.last_frame_at_ms < 2000);
  const receiving = snapshot?.connection === 'connected' && audio?.capture_state === 'receiving' && recent;
  const rms = receiving ? Math.max(0, Math.min(1000, audio?.rms_milli ?? 0)) : 0;
  const db = rms > 0 ? Math.round(20 * Math.log10(rms / 1000)) : null;
  const deadline = snapshot?.phase === 'listening' ? snapshot.listening_deadline_ms : snapshot?.follow_up_deadline_ms;
  const cuePending = (snapshot?.feedback?.kind === 'wake' || snapshot?.feedback?.kind === 'follow_up')
    && (snapshot.feedback.status === 'pending' || snapshot.feedback.status === 'started') && !audio?.speech_detected;
  const cueReason = snapshot?.feedback?.reason_code;
  const cueReasonKey = cueReason === 'feedback_muted' ? 'voice.feedback.reason.muted'
    : cueReason === 'cue_preempted_or_state_changed' ? 'voice.feedback.reason.preempted'
    : cueReason === 'speaker_unavailable' || cueReason === 'speaker_stream_failed' ? 'voice.feedback.reason.device'
    : cueReason === 'earcon_drain_timeout' ? 'voice.feedback.reason.timeout'
    : cueReason ? 'voice.feedback.reason.output' : null;
  const diagnostics = <>
    {audio?.barge_in_reason && <div>{t(audio.barge_in_ready ? 'voice.audio.bargeInTest' : 'voice.audio.halfDuplex')}</div>}
    <div>{t('voice.audio.asr')}: {t(audio ? audio.asr_ready ? 'voice.audio.ready' : 'voice.audio.notReady' : 'voice.audio.unknown')} · {t('voice.audio.kws')}: {t(audio ? audio.kws_ready ? 'voice.audio.ready' : 'voice.audio.notReady' : 'voice.audio.unknown')}</div>
    <div>{t('voice.audio.vad')}: {t(audio?.vad_ready ? 'voice.audio.ready' : 'voice.audio.unknown')} · {t('voice.audio.captured', { seconds: ((audio?.utterance_samples ?? 0) / 16000).toFixed(1) })}</div>
    {snapshot?.feedback && <div>{t('voice.feedback.latestCue')}: {t(`voice.feedback.cue.${snapshot.feedback.kind}`)} · {t(`voice.feedback.receipt.${snapshot.feedback.status}`)}
      {cueReasonKey && <> · {t(cueReasonKey)}</>}
    </div>}
  </>;
  return <div className='flex flex-col gap-2 text-12px text-t-secondary'>
    {snapshot?.phase === 'listening' && <div role='status'>{deadline ? t('voice.session.startWithin', { seconds: Math.max(0, Math.ceil((deadline - now) / 1000)) }) : t(cuePending ? 'voice.session.cueListening' : 'voice.session.recording')}</div>}
    {(snapshot?.phase === 'follow_up' || snapshot?.phase === 'text_follow_up') && deadline && <div>{t('voice.session.followWithin', { seconds: Math.max(0, Math.ceil((deadline - now) / 1000)) })}</div>}
    {(snapshot?.phase === 'follow_up' || snapshot?.phase === 'text_follow_up') && !deadline && <div>{t(cuePending ? 'voice.session.cueListening' : 'voice.session.recording')}</div>}
    <div className='flex justify-between gap-2'>
      <span>{t('voice.audio.level')}</span>
      <span>{db === null ? t('voice.audio.silent') : `${db} dBFS`}</span>
    </div>
    <meter className='w-full' min={0} max={100} value={db === null ? 0 : Math.max(0, Math.min(100, (db + 60) / 60 * 100))} aria-label={t('voice.audio.level')} />
    <div>{t(!audio ? 'voice.audio.waiting' : receiving ? 'voice.audio.receiving' : 'voice.audio.noPcm')} · {t(audio?.speech_detected && receiving ? 'voice.audio.speech' : 'voice.audio.noSpeech')}</div>
    {compact ? <details><summary className='cursor-pointer'>{t('voice.session.audioDetails')}</summary>{diagnostics}</details> : diagnostics}
    {includeError && <VoiceErrorStatus error={snapshot?.error ?? null} endedReason={snapshot?.ended_reason} />}
  </div>;
}
