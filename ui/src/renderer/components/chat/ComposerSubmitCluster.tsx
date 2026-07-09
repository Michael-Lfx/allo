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

  // `filled` (black circle) is reserved for when the mic is the ONLY primary
  // button (empty idle composer). Whenever another circle button is visible
  // (stop while streaming, autowork robot, send with draft), fall back to the
  // gray inline mic so there is never a second black circle.
  const speechVariant = hasDraft || showStop || autoWorkMode ? 'inline' : 'filled';

  return (
    <div className='composer-submit-cluster flex items-center gap-2'>
      <SpeechInputButton
        hidden={hideSpeech}
        disabled={speechDisabled}
        variant={speechVariant}
        locale={speechLocale}
        onTranscript={onSpeechTranscript}
      />

      {showStop && onStop ? (
        <Button
          shape='circle'
          type='secondary'
          className='send-button-custom sendbox-stop-button'
          icon={<div className='sendbox-stop-icon' aria-hidden='true' />}
          onClick={onStop}
          data-testid='composer-stop-btn'
        />
      ) : null}

      {showSteer && steerAvailable && onSteer && hasDraft ? (
        <Button
          shape='circle'
          type='primary'
          disabled={sendDisabled}
          className='send-button-custom sendbox-steer-button'
          title={t('conversation.steer.button')}
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
    </div>
  );
};

export default ComposerSubmitCluster;
