import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import { Up, Voice } from '@icon-park/react';
import classNames from 'classnames';
import type { MeetingSession } from '@/common/adapter/ipcBridge';
import MeetingWaveform from './MeetingWaveform';

type MeetingRecordingDockProps = {
  session: MeetingSession;
  busy: boolean;
  transcriptOpen: boolean;
  onToggleTranscript: () => void;
  onStart: () => void;
  onStop: () => void;
};

const MeetingRecordingDock: React.FC<MeetingRecordingDockProps> = ({
  session,
  busy,
  transcriptOpen,
  onToggleTranscript,
  onStart,
  onStop,
}) => {
  const { t } = useTranslation();
  const isCapturing = session.status === 'recording' || session.status === 'paused';
  const isBusyState = busy || session.status === 'stopping';
  const transcriptLabel = transcriptOpen ? t('meeting.transcript.hide') : t('meeting.transcript.show');

  return (
    <div
      className={classNames('meeting-dock pointer-events-auto relative select-none', isCapturing && 'meeting-dock--live')}
    >
      <div
        className={classNames(
          'absolute inset-0 flex items-center justify-center gap-4px p-5px',
          isCapturing ? 'pointer-events-none opacity-0' : 'opacity-100'
        )}
        aria-hidden={isCapturing}
      >
        <Tooltip content={t('meeting.dock.start')}>
          <button
            type='button'
            className='flex h-32px w-32px shrink-0 items-center justify-center rounded-full text-white/70 transition-colors hover:bg-white/15 hover:text-white disabled:opacity-60'
            aria-label={t('meeting.dock.start')}
            disabled={isBusyState}
            onClick={onStart}
          >
            <Voice theme='outline' size={18} fill='currentColor' />
          </button>
        </Tooltip>
        <Tooltip content={transcriptLabel}>
          <button
            type='button'
            className='flex h-32px w-32px shrink-0 items-center justify-center rounded-full text-white/50 transition-colors hover:bg-white/15 hover:text-white/80'
            aria-label={transcriptLabel}
            onClick={onToggleTranscript}
          >
            <span className={classNames('inline-flex transition-transform duration-200', transcriptOpen && 'rotate-180')}>
              <Up theme='outline' size={14} fill='currentColor' />
            </span>
          </button>
        </Tooltip>
      </div>

      <div
        className={classNames(
          'flex h-full w-full items-center justify-center gap-12px pl-28px pr-20px',
          isCapturing ? 'opacity-100' : 'pointer-events-none opacity-0'
        )}
        aria-hidden={!isCapturing}
      >
        <MeetingWaveform active={session.status === 'recording'} />
        <Tooltip content={t('meeting.dock.stop')}>
          <button
            type='button'
            className='flex shrink-0 items-center justify-center rounded-full p-6px text-[#ef4444] transition-colors hover:bg-white/15 disabled:opacity-60'
            aria-label={t('meeting.dock.stop')}
            disabled={isBusyState}
            onClick={onStop}
          >
            <span className='block size-14px rounded-2px bg-current' />
          </button>
        </Tooltip>
      </div>
    </div>
  );
};

export default MeetingRecordingDock;
