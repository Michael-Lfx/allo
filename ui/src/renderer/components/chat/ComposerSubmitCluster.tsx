/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import SpeechInputButton from '@/renderer/components/chat/SpeechInputButton';
import { useClawAsrAvailable } from '@/renderer/hooks/system/useClawAsrAvailable';
import { autoWorkStartDisabled } from '@/renderer/pages/guid/hooks/autoWorkEntry';
import type { AutoWorkDraftValue } from '@/renderer/pages/conversation/components/AutoWorkControl';
import { Button, Tooltip } from '@arco-design/web-react';
import { ArrowUp, Lightning, Robot } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';

export type ComposerSubmitClusterProps = {
  hasDraft: boolean;
  loading?: boolean;
  disabled?: boolean;
  isUploading?: boolean;
  autoWorkMode?: boolean;
  autoWorkDraft?: AutoWorkDraftValue;
  speechLocale?: string;
  onSend: () => void;
  onSpeechTranscript: (text: string) => void;
  showStop?: boolean;
  onStop?: () => void;
  showSteer?: boolean;
  steerAvailable?: boolean;
  onSteer?: () => void;
  speechHidden?: boolean;
  sendTestId?: string;
};

const ComposerSubmitCluster: React.FC<ComposerSubmitClusterProps> = ({
  hasDraft,
  loading = false,
  disabled = false,
  isUploading = false,
  autoWorkMode = false,
  autoWorkDraft,
  speechLocale,
  onSend,
  onSpeechTranscript,
  showStop = false,
  onStop,
  showSteer = false,
  steerAvailable = false,
  onSteer,
  speechHidden = false,
  sendTestId = 'composer-send-btn',
}) => {
  const { t } = useTranslation();
  const { ready, available } = useClawAsrAvailable();
  const hideSpeech = speechHidden || !ready || !available;

  const speechDisabled = disabled || loading || isUploading || (showStop && !hasDraft);

  const autoWorkDisabled =
    loading || (autoWorkDraft ? autoWorkStartDisabled(loading, autoWorkDraft) : !autoWorkMode);

  const sendDisabled = disabled || isUploading || !hasDraft;

  const showSendButton = hasDraft && !autoWorkMode;
  const showSendWithAutoWork = hasDraft && autoWorkMode;
  const showAutoWorkButton = autoWorkMode;

  // Keep the rightmost circle slot stable: idle composer shows a filled (black)
  // mic there; once send/stop/robot appears that slot stays put and a gray
  // inline mic opens to its left — avoids the mic jumping right→left and
  // swapping black→transparent on the first keystroke.
  const hasCompanionCircle =
    showStop || showAutoWorkButton || showSendButton || showSendWithAutoWork;
  const showSecondarySpeech = !hideSpeech && hasCompanionCircle;
  const showPrimaryFilledSpeech = !hideSpeech && !hasCompanionCircle;

  const speechButtonProps = {
    disabled: speechDisabled,
    locale: speechLocale,
    onTranscript: onSpeechTranscript,
  };

  return (
    <div
      className={`composer-submit-cluster flex items-center gap-2${showSecondarySpeech ? ' composer-submit-cluster--with-secondary-speech' : ''}`}
    >
      {showSecondarySpeech ? (
        <div className='composer-submit-cluster__speech-secondary'>
          <SpeechInputButton {...speechButtonProps} variant='inline' />
        </div>
      ) : null}

      {showStop && onStop ? (
        <Button
          shape='circle'
          type='secondary'
          className='send-button-custom sendbox-stop-button'
          icon={<div className='sendbox-stop-icon' aria-hidden='true' />}
          onClick={onStop}
          data-testid='composer-stop-btn'
          aria-label={t('conversation.chat.stop', { defaultValue: 'Stop' })}
          title={t('conversation.chat.stop', { defaultValue: 'Stop generating' })}
        />
      ) : null}

      {showSteer && steerAvailable && onSteer && hasDraft ? (
        <Button
          shape='circle'
          type='primary'
          disabled={sendDisabled}
          className='send-button-custom sendbox-steer-button'
          title={t('conversation.steer.button')}
          aria-label={t('conversation.steer.button')}
          icon={<Lightning theme='filled' size='14' fill='white' strokeWidth={5} />}
          onClick={onSteer}
          data-testid='composer-steer-btn'
        />
      ) : null}

      {showAutoWorkButton ? (
        <Tooltip content={t('requirements.autowork.startSession')}>
          <Button
            shape='circle'
            type='primary'
            loading={loading}
            disabled={autoWorkDisabled}
            className='send-button-custom'
            icon={<Robot theme='filled' size='14' fill='white' strokeWidth={5} />}
            onClick={onSend}
            data-testid='composer-autowork-btn'
          />
        </Tooltip>
      ) : null}

      {showSendButton ? (
        <Button
          shape='circle'
          type='primary'
          loading={loading}
          disabled={sendDisabled}
          className='send-button-custom'
          icon={<ArrowUp theme='filled' size='14' fill='white' strokeWidth={5} />}
          onClick={onSend}
          data-testid={sendTestId}
          aria-label={
            showSteer
              ? t('conversation.chat.queueNext', { defaultValue: 'Queue as next step' })
              : t('conversation.chat.send', { defaultValue: 'Send' })
          }
          title={
            showSteer
              ? t('conversation.chat.queueNext', { defaultValue: 'Queue as next step' })
              : t('conversation.chat.send', { defaultValue: 'Send' })
          }
        />
      ) : null}

      {showSendWithAutoWork ? (
        <Button
          shape='circle'
          type='primary'
          loading={loading}
          disabled={sendDisabled}
          className='send-button-custom'
          icon={<ArrowUp theme='filled' size='14' fill='white' strokeWidth={5} />}
          onClick={onSend}
          data-testid={sendTestId}
        />
      ) : null}

      {showPrimaryFilledSpeech ? (
        <SpeechInputButton {...speechButtonProps} variant='filled' />
      ) : null}
    </div>
  );
};

export default ComposerSubmitCluster;
