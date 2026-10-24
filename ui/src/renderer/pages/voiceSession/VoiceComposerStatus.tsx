/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { Button } from '@arco-design/web-react';
import { Voice } from '@icon-park/react';
import { AudioLines } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import VoiceTaskQueue from './VoiceTaskQueue';
import VoiceTaskStatus from './VoiceTaskStatus';
import VoiceErrorStatus from './VoiceErrorStatus';
import './voiceCallStatus.css';

/**
 * Conversation-owned compact status pill, positioned in top-left of composer internal row.
 * Height 26px, font-size 12px, icon 14px, role="status".
 */
export default function VoiceComposerStatus({
  conversationId,
  snapshot,
  failed = false,
  historyFailed = false,
  isConnecting = false,
  isActiveElsewhere = false,
  onRetryHistory,
}: {
  conversationId: string;
  snapshot: VoiceSnapshot | null;
  failed?: boolean;
  historyFailed?: boolean;
  isConnecting?: boolean;
  isActiveElsewhere?: boolean;
  onRetryHistory?: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const bound = snapshot?.binding?.conversation_id === conversationId && snapshot.binding.session_id === snapshot.session_id;
  const task = snapshot?.task?.conversation_id === conversationId ? snapshot.task : null;
  const items = snapshot?.queue?.filter((item) => item.task.conversation_id === conversationId && item.position > 0) ?? [];
  const confirmation = bound ? snapshot?.confirmation : null;
  const needsControl = task && (Boolean(confirmation || task.error_code || snapshot?.task_error_code) || ['unknown', 'delivering', 'cancelling'].includes(task.status));
  const scope = `${conversationId}:${snapshot?.instance_id ?? ''}`;

  const isElsewhere = !bound && (isActiveElsewhere || Boolean(snapshot?.binding?.conversation_id && snapshot.session_id && snapshot.phase !== 'idle' && !snapshot.ended_reason));

  let statusLabel: string;
  let statusIcon: React.ReactNode;

  if (failed) {
    statusLabel = t('voice.session.unavailable', { defaultValue: '语音服务暂不可用' });
    statusIcon = <Voice theme='outline' size='14' className='text-danger-6 shrink-0' />;
  } else if (!snapshot) {
    statusLabel = t('voice.composer.pill.syncing');
    statusIcon = <AudioLines size={14} className='text-t-secondary shrink-0' aria-hidden='true' />;
  } else if (isConnecting || snapshot.connection === 'starting') {
    statusLabel = t('voice.composer.pill.connecting', { defaultValue: '连接中' });
    statusIcon = <Voice theme='outline' size='14' className='text-t-secondary shrink-0 animate-spin' />;
  } else if (isElsewhere) {
    statusLabel = t('voice.composer.pill.activeElsewhere', { defaultValue: '别处通话中' });
    statusIcon = <Voice theme='outline' size='14' className='text-t-secondary shrink-0' />;
  } else if (snapshot.connection !== 'connected') {
    statusLabel = t('voice.session.unavailable');
    statusIcon = <Voice theme='outline' size='14' className='text-danger-6 shrink-0' />;
  } else if (snapshot.ended_reason) {
    statusLabel = t('voice.session.phase.idle');
    statusIcon = <AudioLines size={14} className='text-t-secondary shrink-0' aria-hidden='true' />;
  } else if (bound && !snapshot.acoustic_ready && ['listening', 'follow_up'].includes(snapshot.phase)) {
    statusLabel = t('voice.session.phase.text_follow_up');
    statusIcon = <AudioLines size={14} className='text-t-secondary shrink-0' aria-hidden='true' />;
  } else if (bound && snapshot) {
    statusLabel = t(`voice.session.phase.${snapshot.phase}`);
    statusIcon = ['listening', 'follow_up', 'speaking'].includes(snapshot.phase)
      ? <AudioLines size={14} className='voice-call-wave text-primary-6 shrink-0' aria-hidden='true' />
      : <Voice theme='outline' size='14' className={['listening', 'transcribing', 'deciding', 'synthesizing'].includes(snapshot.phase) ? 'text-primary-6 shrink-0' : 'text-t-secondary shrink-0'} />;
  } else {
    statusLabel = t('voice.session.phase.idle');
    statusIcon = <Voice theme='outline' size='14' className='text-t-secondary shrink-0' />;
  }

  return (
    <div className='flex flex-wrap items-center gap-6px text-12px select-none' data-testid='voice-composer-status'>
      <div
        className='inline-flex items-center gap-6px h-26px px-8px rd-full bg-fill-2 b-1px b-solid border-arco-2 text-12px'
        role='status'
        aria-live='polite'
      >
        {statusIcon}
        <span className='font-normal text-t-primary'>{statusLabel}</span>
      </div>

      {needsControl && (
        <details key={`${scope}:${task.command_id}:${confirmation?.approval_id ?? ''}`} open={Boolean(confirmation)} className='inline-flex items-center'>
          <summary className='cursor-pointer text-primary-6 hover:underline px-6px py-2px rd-4px bg-fill-1 b-1px b-solid border-arco-2'>
            {t(confirmation ? 'voice.task.confirmationTitle' : 'voice.task.status.' + task.status)}
          </summary>
          <VoiceTaskStatus
            compact
            currentConversationId={conversationId}
            task={task}
            showResult={false}
            errorCode={snapshot?.task_error_code}
            instanceId={snapshot?.instance_id}
            confirmation={confirmation}
          />
        </details>
      )}

      {items.length > 0 && (
        <details key={scope} className='inline-flex items-center'>
          <summary className='cursor-pointer text-t-secondary hover:text-t-primary px-6px py-2px rd-4px bg-fill-1 b-1px b-solid border-arco-2'>
            {t('voice.composer.queued', { count: items.length })}
          </summary>
          <VoiceTaskQueue items={items} instanceId={snapshot?.instance_id} conversationId={conversationId} />
        </details>
      )}

      {bound && snapshot?.error && <VoiceErrorStatus error={snapshot.error} />}

      {historyFailed && (
        <div role='alert' className='inline-flex items-center gap-4px px-6px py-2px rd-4px bg-[rgba(var(--danger-6),0.08)] text-danger-6'>
          <span>{t('voice.composer.historyFailed')}</span>
          <Button
            type='text'
            size='mini'
            className='p-0 text-12px text-danger-6 hover:underline'
            onClick={() => {
              void onRetryHistory?.();
            }}
          >
            {t('voice.composer.retryHistory')}
          </Button>
        </div>
      )}
    </div>
  );
}
