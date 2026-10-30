

import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import SpeechInputButton, { type DictationActivationRequest } from '@/renderer/components/chat/SpeechInputButton';
import { useClawAsrAvailable } from '@/renderer/hooks/system/useClawAsrAvailable';
import { autoWorkStartDisabled } from '@/renderer/pages/guid/hooks/autoWorkEntry';
import type { AutoWorkDraftValue } from '@/renderer/pages/conversation/components/AutoWorkControl';
import { Button, Tooltip } from '@arco-design/web-react';
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
  dictationActivation?: DictationActivationRequest;
  showStop?: boolean;
  onStop?: (clickDetail: number) => void;
  stopPending?: boolean;
  showSteer?: boolean;
  steerAvailable?: boolean;
  onSteer?: () => void;
  speechHidden?: boolean;
  isVoiceConversation?: boolean;
  isVoiceActive?: boolean;
  isVoiceActiveElsewhere?: boolean;
  isVoiceConnecting?: boolean;
  isVoiceStartUncertain?: boolean;
  voiceCallLabel?: string;
  onStartVoiceCall?: () => void;
  onStopVoiceCall?: () => void;
  phoneCallTestId?: string;
  sendTestId?: string;
  stopTestId?: string;
};

export type ComposerStopButtonProps = {
  label: string;
  title: string;
  onStop: (clickDetail: number) => void;
  pending?: boolean;
  testId?: string;
};

const readClickDetail = (event: Event): number => {
  const detail = (event as Event & { detail?: unknown }).detail;
  return typeof detail === 'number' ? detail : 0;
};

export const ComposerStopButton: React.FC<ComposerStopButtonProps> = ({
  label,
  title,
  onStop,
  pending = false,
  testId = 'composer-stop-btn',
}) => (
  <Button
    shape='circle'
    type='secondary'
    className='send-button-custom sendbox-stop-button'
    icon={<div className='sendbox-stop-icon' aria-hidden='true' />}
    onClick={(event) => onStop(readClickDetail(event))}
    disabled={pending}
    loading={pending}
    data-testid={testId}
    data-layout-part='send'
    aria-label={label}
    title={title}
  />
);

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
  dictationActivation,
  showStop = false,
  onStop,
  stopPending = false,
  showSteer = false,
  steerAvailable = false,
  onSteer,
  speechHidden = false,
  isVoiceConversation = false,
  isVoiceActive = false,
  isVoiceActiveElsewhere = false,
  isVoiceConnecting = false,
  isVoiceStartUncertain = false,
  voiceCallLabel,
  onStartVoiceCall,
  onStopVoiceCall,
  phoneCallTestId = 'composer-phone-btn',
  sendTestId = 'composer-send-btn',
  stopTestId = 'composer-stop-btn',
}) => {
  const { t } = useTranslation();
  const { ready, available } = useClawAsrAvailable();
  const hideSpeech = isVoiceConversation || speechHidden || !ready || !available;

  const speechDisabled = disabled || loading || isUploading || (showStop && !hasDraft);

  const autoWorkDisabled =
    loading || (autoWorkDraft ? autoWorkStartDisabled(loading, autoWorkDraft) : !autoWorkMode);

  const sendDisabled = disabled || isUploading || !hasDraft;
  const showStopButton = showStop && Boolean(onStop);
  const showPhoneButton = isVoiceConversation && !showStopButton && (!hasDraft || isVoiceConnecting || isVoiceStartUncertain);

  // A running conversation replaces Send with Stop. Outside that state, Send
  // remains visible and is disabled until the draft is valid.
  const showSendButton = !autoWorkMode && !showStopButton;
  const effectiveShowSendButton = isVoiceConversation ? hasDraft && !showStopButton && !isVoiceConnecting && !isVoiceStartUncertain : showSendButton;
  const showSteerButton = showSteer && !showStopButton;
  const effectiveShowSteerButton = !isVoiceConversation && showSteerButton;
  const showAutoWorkButton = !isVoiceConversation && autoWorkMode;

  // Keep the rightmost circle slot stable: idle composer shows a filled (black)
  // mic there; once send/stop/robot appears that slot stays put and a gray
  // inline mic opens to its left — avoids the mic jumping right→left and
  // swapping black→transparent on the first keystroke.
  const hasCompanionCircle =
    showPhoneButton || showStopButton || showAutoWorkButton || effectiveShowSendButton;
  const showSecondarySpeech = !hideSpeech && hasCompanionCircle;
  const showPrimaryFilledSpeech = !hideSpeech && !hasCompanionCircle;

  const speechButtonProps = {
    disabled: speechDisabled,
    locale: speechLocale,
    onTranscript: onSpeechTranscript,
    activationRequest: dictationActivation,
  };

  return (
    <div
      className={`composer-submit-cluster flex items-center gap-2${showSecondarySpeech ? ' composer-submit-cluster--with-secondary-speech' : ''}`}
      data-layout-slot='submit'
    >
      {showSecondarySpeech ? (
        <div className='composer-submit-cluster__speech-secondary'>
          <SpeechInputButton {...speechButtonProps} variant='inline' />
        </div>
      ) : null}

        {isVoiceConversation && isVoiceActive && (showStopButton || effectiveShowSendButton) && onStopVoiceCall && (
          <Button size='mini' type='text' status='danger' icon={<FlowyIcon name='phoneOff' size={14} />}
            onClick={onStopVoiceCall} data-testid='composer-secondary-hangup'
            aria-label={t('voice.session.hangup')} title={t('voice.session.hangup')}/>
        )}
      <div className='composer-submit-cluster__primary'>
        {showStopButton && onStop ? (
          <ComposerStopButton
            onStop={onStop}
            pending={stopPending}
            testId={stopTestId}
            label={t('conversation.chat.stop', { defaultValue: 'Stop' })}
            title={t('conversation.chat.stop', { defaultValue: 'Stop generating' })}
          />
        ) : null}

        {effectiveShowSteerButton && steerAvailable && onSteer && hasDraft ? (
          <Button
            shape='circle'
            type='primary'
            disabled={sendDisabled}
            className='send-button-custom sendbox-steer-button'
            title={t('conversation.steer.button')}
            aria-label={t('conversation.steer.button')}
            icon={<FlowyIcon name='lightning' size={14} weight='fill' />}
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
              icon={<FlowyIcon name='robot' size={14} weight='fill' />}
              onClick={onSend}
              data-testid='composer-autowork-btn'
            />
          </Tooltip>
        ) : null}

        {effectiveShowSendButton ? (
          <Button
            shape='circle'
            type='primary'
            loading={loading}
            disabled={sendDisabled}
            className='send-button-custom'
            icon={<FlowyIcon name='arrowUp' size={14} weight='fill' />}
            onClick={onSend}
            data-testid={sendTestId}
            data-layout-part='send'
            aria-label={
              showSteer
                ? t('conversation.chat.queueNext', { defaultValue: 'Queue as next step' })
                : isVoiceConversation
                  ? t('voice.session.sendText', { defaultValue: '发送文字' })
                  : t('conversation.chat.send', { defaultValue: 'Send' })
            }
            title={
              showSteer
                ? t('conversation.chat.queueNext', { defaultValue: 'Queue as next step' })
                : isVoiceConversation
                  ? t('voice.session.sendText', { defaultValue: '发送文字' })
                  : t('conversation.chat.send', { defaultValue: 'Send' })
            }
          />
        ) : null}

        {showPhoneButton ? (
          isVoiceActive ? (
            <Button
              shape='circle'
              type='primary'
              loading={isVoiceConnecting}
              status='danger'
              disabled={isVoiceConnecting}
              className='send-button-custom sendbox-phone-hangup-button'
              icon={<FlowyIcon name='phoneOff' size={14} weight='fill' />}
              onClick={onStopVoiceCall}
              data-testid={phoneCallTestId}
              data-layout-part='phone'
              aria-label={t('voice.session.hangup', { defaultValue: '挂断通话' })}
              title={t('voice.session.hangup', { defaultValue: '挂断通话' })}
            />
          ) : (
            <Button
              shape='circle'
              type='primary'
              loading={isVoiceConnecting}
              className='send-button-custom sendbox-phone-call-button'
              disabled={isVoiceConnecting}
              icon={<FlowyIcon name='phone' size={14} weight='fill' />}
              onClick={onStartVoiceCall}
              data-testid={phoneCallTestId}
              data-layout-part='phone'
              aria-label={voiceCallLabel || t(isVoiceStartUncertain ? 'voice.composer.retryStart' : isVoiceActiveElsewhere ? 'voice.entry.openActive' : 'voice.session.startCall')}
              title={voiceCallLabel || t(isVoiceStartUncertain ? 'voice.composer.retryStart' : isVoiceActiveElsewhere ? 'voice.entry.openActive' : 'voice.session.startCall')}
            />
          )
        ) : null}

        {showPrimaryFilledSpeech ? (
          <SpeechInputButton {...speechButtonProps} variant='filled' />
        ) : null}
      </div>
    </div>
  );
};

export default ComposerSubmitCluster;
