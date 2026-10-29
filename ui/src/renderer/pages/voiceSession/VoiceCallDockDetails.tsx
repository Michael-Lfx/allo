/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { Button } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import VoiceErrorStatus from './VoiceErrorStatus';
import VoiceTaskQueue from './VoiceTaskQueue';
import VoiceTaskStatus from './VoiceTaskStatus';
import type { VoiceDockDetails } from './voiceCallDockState';

export default function VoiceCallDockDetails({
  conversationId,
  snapshot,
  details,
  onRetryHistory,
}: {
  conversationId: string;
  snapshot: VoiceSnapshot | null;
  details: VoiceDockDetails;
  onRetryHistory?: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const hasTask = Boolean(details.task);
  const hasQueue = details.queue.length > 0;
  const hasError = Boolean(details.error || details.endedReason);
  if (!hasTask && !hasQueue && !details.historyFailed && !hasError) return null;

  return (
    <div className='voice-call-dock-details__body'>
      {hasTask && (
        <section className='voice-call-dock-details__section' aria-label={t('voice.task.title')}>
          <h3 className='voice-call-dock-details__title'>{t('voice.task.title')}</h3>
          <VoiceTaskStatus
            compact
            currentConversationId={conversationId}
            task={details.task}
            showResult
            errorCode={snapshot?.task_error_code}
            instanceId={snapshot?.instance_id}
            confirmation={details.confirmation}
          />
        </section>
      )}
      {hasQueue && (
        <div className='voice-call-dock-details__section'>
          <VoiceTaskQueue
            items={details.queue}
            instanceId={snapshot?.instance_id}
            conversationId={conversationId}
          />
        </div>
      )}
      {details.historyFailed && (
        <div className='voice-call-dock-details__recovery' role='alert'>
          <span>{t('voice.composer.historyFailed')}</span>
          <Button
            type='text'
            size='mini'
            className='voice-call-dock-details__retry'
            onClick={() => {
              void onRetryHistory?.();
            }}
          >
            {t('voice.composer.retryHistory')}
          </Button>
        </div>
      )}
      {details.error && <VoiceErrorStatus error={details.error} />}
      {!details.error && details.endedReason && (
        <VoiceErrorStatus error={null} endedReason={details.endedReason} />
      )}
    </div>
  );
}
