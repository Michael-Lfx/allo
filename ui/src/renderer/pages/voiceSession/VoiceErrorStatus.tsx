import React from 'react';
import { useTranslation } from 'react-i18next';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';

export function voiceErrorKey(code: string) {
  const keys = {
    voice_turn_failed: 'voice.audio.turnFailed',
    task_not_found:'voice.task.noActive',task_status_unavailable:'voice.task.controlUnknown',task_receipt_unavailable:'voice.progress.unverified',
    confirmation_audio_or_timeout: 'voice.task.confirmationFailed', task_approval_failed: 'voice.task.confirmationFailed', task_approval_changed:'voice.task.scopeChanged',
    task_approval_detail_required:'voice.task.confirmationDetailRequired',
    execution_unconfigured: 'voice.execution.unavailable', task_workspace_invalid: 'voice.execution.failed',
    task_execution_model_unconfigured: 'voice.execution.failed', task_preset_unavailable: 'voice.execution.failed', task_preset_requires_nomi: 'voice.execution.failed',
    task_delivery_unknown: 'voice.task.status.unknown', task_creation_unknown: 'voice.task.status.unknown',
    task_scope_changed: 'voice.task.scopeChanged', task_command_conflict: 'voice.task.scopeChanged', task_receipt_scope_changed: 'voice.task.status.unknown',
    task_already_active: 'voice.task.active', task_goal_invalid: 'voice.audio.modelInvalid',
    task_failed: 'voice.task.status.failed', task_state_write_failed: 'voice.task.persistenceFailed',
    task_control_unknown: 'voice.task.controlUnknown', task_cancel_failed: 'voice.task.controlUnknown',
    task_delivery_pending: 'voice.task.status.delivering', task_result_unverified: 'voice.task.status.unknown',
    task_receipt_missing: 'voice.task.status.unknown', task_creation_rejected: 'voice.task.failedHint',
    task_delivery_rejected: 'voice.task.failedHint', task_policy_missing: 'voice.task.failedHint',
    capture_no_pcm: 'voice.audio.captureNoPcm', capture_closed: 'voice.audio.captureClosed',
    speech_not_detected: 'voice.audio.speechNotDetected', vad_model_missing: 'voice.audio.vadMissing', vad_model_failed: 'voice.audio.vadFailed',
    wake_only: 'voice.audio.wakeOnly',
    asr_empty_result: 'voice.audio.asrEmpty', model_unavailable: 'voice.audio.dialogueMissing',
    model_network_failed: 'voice.audio.modelNetwork', model_auth_failed: 'voice.audio.modelAuth', model_rejected: 'voice.audio.modelRejected', model_response_invalid: 'voice.audio.modelInvalid',
    model_timeout:'voice.audio.modelTimeout', binding_invalid:'voice.audio.bindingInvalid', binding_busy:'voice.audio.bindingBusy',
    voice_record_conflict:'voice.audio.recordConflict', voice_record_write_failed:'voice.audio.recordFailed', voice_projection_write_failed:'voice.audio.projectionFailed', voice_generation_unknown:'voice.audio.generationUnknown',
    tts_failed: 'voice.audio.ttsFailed', tts_rate_limited: 'voice.audio.ttsRateLimited', tts_empty_audio: 'voice.audio.ttsEmpty', speaker_failed: 'voice.audio.speakerFailed', runtime_unavailable: 'voice.audio.taskMissing',
  } as const;
  return keys[code as keyof typeof keys] ?? 'voice.session.unavailable';
}

export default function VoiceErrorStatus({ error, endedReason }: { error: VoiceSnapshot['error']; endedReason?: VoiceSnapshot['ended_reason'] }) {
  const { t } = useTranslation();
  if (!error) {
    if (!endedReason || endedReason === 'failed') return null;
    return <p role='status' className='text-12px text-t-secondary whitespace-pre-wrap m-0'>{t(`voice.session.ended.${endedReason}`)}</p>;
  }
  return <p role='alert' className='text-12px text-t-secondary whitespace-pre-wrap m-0'>{t(voiceErrorKey(error.code))}</p>;
}
