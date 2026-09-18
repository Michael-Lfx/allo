import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import './meetingCaptions.css';

type MeetingCaptionsPayload = {
  visible: boolean;
  text: string;
  speaker: string;
  is_partial: boolean;
  phase: string;
};

type CaptionLine = {
  text: string;
  speaker: string;
  is_partial: boolean;
};

const MeetingCaptionsPage: React.FC = () => {
  const { t } = useTranslation();
  const [line, setLine] = useState<CaptionLine | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen<MeetingCaptionsPayload>('meeting-captions://update', (event) => {
        if (disposed) return;
        const payload = event.payload;
        if (!payload.visible || !payload.text.trim()) {
          setLine(null);
          return;
        }
        setLine({
          text: payload.text,
          speaker: payload.speaker,
          is_partial: payload.is_partial,
        });
      });
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!line?.text.trim()) {
    return <main className='meeting-captions meeting-captions--empty' />;
  }

  return (
    <main className='meeting-captions'>
      <section
        className={
          line.is_partial
            ? 'meeting-captions__card meeting-captions__card--partial'
            : 'meeting-captions__card'
        }
        aria-live='polite'
      >
        <div className='meeting-captions__row'>
          {line.speaker ? <span className='meeting-captions__speaker'>{line.speaker}</span> : null}
          {line.is_partial ? (
            <span className='meeting-captions__live' aria-hidden>
              <i />
              <i />
              <i />
            </span>
          ) : null}
          {line.is_partial ? (
            <span className='meeting-captions__badge'>{t('meeting.partial')}</span>
          ) : null}
        </div>
        <span className='meeting-captions__text'>{line.text}</span>
      </section>
    </main>
  );
};

export default MeetingCaptionsPage;
