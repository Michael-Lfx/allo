import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Pause, Play, Up, Voice } from '@icon-park/react';
import classNames from 'classnames';
import type { MeetingSession } from '@/common/adapter/ipcBridge';
import MeetingWaveform from './MeetingWaveform';
import { formatDurationMs, isLiveSession } from './format';

type MeetingRecordingDockProps = {
  session: MeetingSession;
  elapsedMs: number;
  busy: boolean;
  transcriptOpen: boolean;
  onToggleTranscript: () => void;
  onStart: () => void;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
};

const TranscriptToggle: React.FC<{
  open: boolean;
  onToggle: () => void;
  label: string;
}> = ({ open, onToggle, label }) => (
  <Tooltip content={label}>
    <button
      type='button'
      className='flex h-32px w-32px items-center justify-center rounded-full text-white/55 transition-colors hover:bg-white/12 hover:text-white/85'
      aria-label={label}
      onClick={onToggle}
    >
      <span className={classNames('inline-flex transition-transform duration-200', open && 'rotate-180')}>
        <Up theme='outline' size={14} fill='currentColor' />
      </span>
    </button>
  </Tooltip>
);

const MeetingRecordingDock: React.FC<MeetingRecordingDockProps> = ({
  session,
  elapsedMs,
  busy,
  transcriptOpen,
  onToggleTranscript,
  onStart,
  onPause,
  onResume,
  onStop,
}) => {
  const { t } = useTranslation();
  const isLive = isLiveSession(session);
  const canStart = session.status === 'created';
  const isRecording = session.status === 'recording';
  const isPaused = session.status === 'paused';
  const isBusyState = busy || session.status === 'stopping';
  const transcriptLabel = transcriptOpen ? t('meeting.transcript.hide') : t('meeting.transcript.show');

  return (
    <div
      className={classNames(
        'pointer-events-auto flex h-42px items-center gap-4px rounded-28px px-6px',
        'bg-[rgba(12,14,18,0.82)] shadow-[0_8px_28px_rgba(0,0,0,0.35)] backdrop-blur-12px',
        'ring-1 ring-[rgba(255,255,255,0.12)]'
      )}
    >
      {isLive ? (
        <>
          <div className='flex items-center gap-10px pl-10px pr-4px'>
            <MeetingWaveform active={isRecording} />
            <span className='min-w-42px text-12px tabular-nums text-white/80'>
              {formatDurationMs(elapsedMs)}
            </span>
          </div>
          {isPaused ? (
            <Tooltip content={t('meeting.dock.resume')}>
              <button
                type='button'
                className='flex h-32px w-32px items-center justify-center rounded-full text-white/80 transition-colors hover:bg-white/12 hover:text-white disabled:opacity-50'
                aria-label={t('meeting.dock.resume')}
                disabled={isBusyState}
                onClick={onResume}
              >
                <Play theme='filled' size={16} fill='currentColor' />
              </button>
            </Tooltip>
          ) : (
            <Tooltip content={t('meeting.dock.pause')}>
              <button
                type='button'
                className='flex h-32px w-32px items-center justify-center rounded-full text-white/80 transition-colors hover:bg-white/12 hover:text-white disabled:opacity-50'
                aria-label={t('meeting.dock.pause')}
                disabled={!isRecording || isBusyState}
                onClick={onPause}
              >
                <Pause theme='filled' size={16} fill='currentColor' />
              </button>
            </Tooltip>
          )}
          <Tooltip content={t('meeting.dock.stop')}>
            <button
              type='button'
              className='flex h-32px w-32px items-center justify-center rounded-full text-white/80 transition-colors hover:bg-white/12 disabled:opacity-50'
              aria-label={t('meeting.dock.stop')}
              disabled={isBusyState}
              onClick={onStop}
            >
              <span className='block size-12px rounded-2px bg-[#ef4444]' />
            </button>
          </Tooltip>
        </>
      ) : canStart ? (
        <Tooltip content={t('meeting.dock.start')}>
          <button
            type='button'
            className='flex h-32px w-32px items-center justify-center rounded-full text-white/75 transition-colors hover:bg-white/12 hover:text-white disabled:opacity-50'
            aria-label={t('meeting.dock.start')}
            disabled={isBusyState}
            onClick={onStart}
          >
            <Voice theme='outline' size={18} fill='currentColor' />
          </button>
        </Tooltip>
      ) : null}
      <TranscriptToggle open={transcriptOpen} onToggle={onToggleTranscript} label={transcriptLabel} />
    </div>
  );
};

export default MeetingRecordingDock;
