

import { Button, Tooltip } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import React, { useEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { RollingDuration } from '@renderer/components/base/Counter';
import {
  useSpeechInput,
  type SpeechInputAvailability,
  type SpeechInputErrorCode,
} from '@/renderer/hooks/system/useSpeechInput';

export type DictationActivationResult = 'started' | 'already-recording' | 'unavailable' | 'stale';

export type DictationActivationRequest = {
  id: string;
  /** Atomically consumes the route-owned intent; false means expired/revoked. */
  claim: () => boolean;
  onResult: (result: DictationActivationResult) => void;
};

type SpeechInputButtonProps = {
  disabled?: boolean;
  hidden?: boolean;
  activationRequest?: DictationActivationRequest;
  /** `filled` = black circle (empty composer); `inline` = gray icon beside send */
  variant?: 'filled' | 'inline';
  locale?: string;
  onTranscript: (transcript: string) => void;
};

const SpeechMicIcon = () => (
  <svg width='18' height='18' viewBox='0 0 24 24' fill='none' stroke='currentColor' strokeWidth='2' aria-hidden='true'>
    <path d='M12 3a3 3 0 0 1 3 3v6a3 3 0 1 1-6 0V6a3 3 0 0 1 3-3Z' />
    <path d='M19 10v2a7 7 0 0 1-14 0v-2' />
    <path d='M12 19v3' />
  </svg>
);

const SpeechStopIcon = () => (
  <svg width='18' height='18' viewBox='0 0 24 24' fill='currentColor' aria-hidden='true'>
    <rect x='6' y='6' width='12' height='12' rx='2.5' />
  </svg>
);

const SpeechLoaderIcon = () => <span className='speech-loader-spinner' aria-hidden='true' />;

const getAvailabilityMessageKey = (availability: SpeechInputAvailability) => {
  return availability === 'record'
    ? 'conversation.chat.speech.recordTooltip'
    : 'conversation.chat.speech.unsupported';
};

const getErrorMessageKey = (errorCode: SpeechInputErrorCode) => {
  switch (errorCode) {
    case 'audio-capture':
      return 'conversation.chat.speech.audioCaptureError';
    case 'empty-transcript':
      return 'conversation.chat.speech.emptyTranscript';
    case 'file-too-large':
      return 'conversation.chat.speech.fileTooLarge';
    case 'network':
      return 'conversation.chat.speech.networkError';
    case 'not-configured':
      return 'conversation.chat.speech.notConfigured';
    case 'permission-denied':
      return 'conversation.chat.speech.permissionDenied';
    case 'recording-unsupported':
      return 'conversation.chat.speech.recordingUnsupported';
    case 'transcription-failed':
      return 'conversation.chat.speech.transcriptionFailed';
    case 'aborted':
    case 'unknown':
    default:
      return 'conversation.chat.speech.genericError';
  }
};

const getTooltipKey = (availability: SpeechInputAvailability, isListening: boolean, isProcessing: boolean) => {
  if (isProcessing) {
    return 'conversation.chat.speech.processing';
  }
  if (isListening) {
    return 'conversation.chat.speech.stopTooltip';
  }
  if (availability === 'record') {
    return 'conversation.chat.speech.recordTooltip';
  }
  return getAvailabilityMessageKey(availability);
};

const SpeechInputButton: React.FC<SpeechInputButtonProps> = ({
  disabled,
  hidden,
  variant = 'filled',
  locale,
  onTranscript,
  activationRequest,
}) => {
  const { t, i18n } = useTranslation();
  const {
    availability,
    clearError,
    errorCode,
    errorMessage,
    recordingDurationMs,
    recordingLevels,
    startRecording,
    status,
    stopRecording,
  } = useSpeechInput({
    locale,
    onTranscript,
  });

  const handledActivation = useRef<string | null>(null);
  const isRecording = status === 'recording';
  const isProcessing = status === 'transcribing';
  const showSpeechFeedback = isRecording || isProcessing;
  const displayedWaveformLevels = useMemo(() => {
    if (recordingLevels.length > 0) {
      return recordingLevels;
    }
    return [0.08, 0.12, 0.1, 0.16, 0.09, 0.14];
  }, [recordingLevels]);

  useEffect(() => {
    if (!activationRequest || handledActivation.current === activationRequest.id) return;
    const finish = (result: DictationActivationResult) => {
      handledActivation.current = activationRequest.id;
      activationRequest.onResult(result);
    };
    if (hidden || disabled || availability !== 'record' || status === 'transcribing') {
      finish('unavailable');
      return;
    }
    if (status === 'recording') {
      finish('already-recording');
      return;
    }
    if (!activationRequest.claim()) {
      finish('stale');
      return;
    }
    finish('started');
    void startRecording();
  }, [activationRequest, availability, disabled, hidden, startRecording, status]);

  useEffect(() => {
    if (!errorCode) {
      return;
    }

    const baseMessage = t(getErrorMessageKey(errorCode));
    const detail = errorMessage?.trim();
    if (errorCode === 'empty-transcript') {
      Message.warning(baseMessage);
      clearError();
      return;
    }
    Message.error(detail ? `${baseMessage}: ${detail}` : baseMessage);
    clearError();
  }, [clearError, errorCode, errorMessage, t]);

  const handleClick = () => {
    if (disabled) {
      return;
    }

    if (availability === 'unsupported') {
      Message.warning(t(getAvailabilityMessageKey(availability)));
      return;
    }

    if (isRecording) {
      stopRecording();
      return;
    }

    void startRecording();
  };

  if (hidden) {
    return null;
  }

  const tooltipKey = getTooltipKey(availability, isRecording, isProcessing);
  const ariaLabel = t(tooltipKey);
  const icon = isRecording ? <SpeechStopIcon /> : isProcessing ? <SpeechLoaderIcon /> : <SpeechMicIcon />;
  const isButtonDisabled = disabled || isProcessing;
  const speechButton = (
    <Button
      type='text'
      size='small'
      shape='circle'
      className={`speech-input-button flowy-button-icon speech-input-button--${variant} ${isRecording ? 'speech-input-button--listening' : ''} ${isProcessing ? 'speech-input-button--processing' : ''}`}
      disabled={isButtonDisabled}
      onClick={handleClick}
      aria-label={ariaLabel}
      data-layout-part='microphone'
      icon={icon}
    />
  );
  const speechButtonTarget = isButtonDisabled ? (
    <span className='speech-input-button-tooltip-target'>{speechButton}</span>
  ) : (
    speechButton
  );

  return (
    <div
      className={`speech-input-control speech-input-control--${variant} ${showSpeechFeedback ? 'speech-input-control--active' : ''}`}
    >
      {showSpeechFeedback && (
        <div
          className={`speech-input-feedback ${isProcessing ? 'speech-input-feedback--processing' : ''}`}
          role='status'
          aria-live='polite'
        >
          <div className='speech-input-feedback__waveform' aria-hidden='true'>
            {displayedWaveformLevels.map((level, index) => (
              <span
                key={`speech-wave-${index}`}
                className='speech-input-feedback__bar'
                style={{
                  height: `${Math.max(1.5, 1 + level * 18)}px`,
                  animationDelay: `${index * 40}ms`,
                }}
              />
            ))}
          </div>
          <span className='speech-input-feedback__label'>
            {isProcessing
              ? t('conversation.chat.speech.transcribingShort')
              : (
                  <RollingDuration
                    seconds={recordingDurationMs / 1000}
                    hours='never'
                    padMinutes={false}
                    locale={locale || i18n.language}
                  />
                )}
          </span>
        </div>
      )}
      <Tooltip content={ariaLabel} mini>{speechButtonTarget}</Tooltip>
    </div>
  );
};

export default SpeechInputButton;
